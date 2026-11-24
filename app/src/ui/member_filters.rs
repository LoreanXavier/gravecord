//! The members table's other column filters (module 920360): Member Since and Joined Discord (`eu` and `en`, both the
//! date menu `el`), Roles (`eV`) and Signals (`ec`), kept as module 406318's search state and sent by module 636496.
//!
//! The date menus offer All and six presets (`moment().subtract(...)` when picked), then Custom Range: a second view
//! with After Date and Before Date, each opening the calendar of module 939538 (`calendarContainer_K-iqS8`). A date
//! filters only once it has an after date (`null != afterDate`), as module 636496 reads it. Roles lists the server's
//! roles but @everyone, highest first, narrowed by module 91871's in-order match; Signals toggles the four safety flags.

use super::motion::{PopKind, PopSide, Popout as _};
use super::server_members::MANAGE_GUILD;
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use chrono::{DateTime, Datelike, Local, Months, NaiveDate, TimeZone, Timelike};
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, div,
    prelude::*, px,
};
use serde_json::{Map, Value, json};

/// `et` of module 920360: the options' ids, `CUSTOM` last.
pub(super) const CUSTOM: u8 = 7;
/// A preset of `el`'s `g`: its id, how far back it reaches (`input`, `unit`) and its label.
type Preset = (u8, Option<(u32, char)>, &'static str);
const PRESETS: [Preset; 7] = [
    (0, None, "jelCib"),
    (1, Some((1, 'h')), "91RDqi"),
    (2, Some((24, 'h')), "Lj/1Tq"),
    (3, Some((7, 'd')), "NnUMSZ"),
    (4, Some((2, 'w')), "hY3XWH"),
    (5, Some((4, 'w')), "kQTwT0"),
    (6, Some((3, 'M')), "EPuP0s"),
];
/// `tX` of module 70738: unusual DM activity counts for two days.
const UNUSUAL_DM_MS: i64 = 2 * 86_400_000;
/// Snowflakes count from 2015 (module 811091).
const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;
/// The open menu's box and its calendar's, for placing the calendar and telling a press inside them.
const MENU_ANCHOR: &str = "server-members-filter-menu";
/// `submenu_frame` registers its box as this.
const CALENDAR_ANCHOR: &str = "submenu";
/// The picker of `pickers::calendar` (`calendarPicker_7yAVe6 .react-datepicker`): 322 wide, its six weeks as tall as
/// the Create Event and Schedule Message pickers measure it.
const CALENDAR_WIDTH: f32 = 322.;
const CALENDAR_HEIGHT: f32 = 364.8;

/// The anchor of the After Date (`true`) or Before Date row.
fn date_row(after: bool) -> &'static str {
    if after {
        "server-members-filter-after-date"
    } else {
        "server-members-filter-before-date"
    }
}

/// Which column's menu is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FilterMenu {
    /// Member Since (`joined-date`).
    Joined,
    /// Joined Discord (`account-age`).
    Account,
    Roles,
    Signals,
}

impl FilterMenu {
    /// The anchor under which the menu opens.
    fn anchor(self) -> &'static str {
        match self {
            Self::Joined => "server-members-filter-joined-date",
            Self::Account => "server-members-filter-account-age",
            Self::Roles => "server-members-filter-roles",
            Self::Signals => "server-members-filter-signals",
        }
    }

    /// `menuName` of `el`.
    fn date_name(self) -> &'static str {
        if self == Self::Joined {
            "joined-date"
        } else {
            "account-age"
        }
    }
}

/// `selectedJoinDateOption` / `selectedAccountAgeOption`: the option and its bounds in epoch milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct DateFilter {
    pub(super) option: u8,
    pub(super) after: Option<i64>,
    pub(super) before: Option<i64>,
}

impl DateFilter {
    fn custom(&self) -> bool {
        self.option == CUSTOM
    }
}

/// Module 406318's search state beyond the text, the sort and the join method.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Filters {
    pub(super) joined: DateFilter,
    pub(super) account: DateFilter,
    /// `selectedRoleIds`, a JS `Set`: in the order they were ticked.
    pub(super) roles: Vec<Id>,
    pub(super) unusual_dm: bool,
    pub(super) timed_out: bool,
    pub(super) unusual_account: bool,
    pub(super) quarantined: bool,
}

impl Filters {
    /// `x` of `eL`: a safety flag is on.
    fn signals(&self) -> bool {
        self.unusual_dm || self.timed_out || self.unusual_account || self.quarantined
    }

    fn date(&self, menu: FilterMenu) -> &DateFilter {
        if menu == FilterMenu::Joined {
            &self.joined
        } else {
            &self.account
        }
    }

    fn date_mut(&mut self, menu: FilterMenu) -> &mut DateFilter {
        if menu == FilterMenu::Joined {
            &mut self.joined
        } else {
            &mut self.account
        }
    }

    /// The header's filtered look (`j`, `V`, `f`, `x` of `eL`).
    pub(super) fn filtered(&self, menu: FilterMenu) -> bool {
        match menu {
            FilterMenu::Joined | FilterMenu::Account => self.date(menu).after.is_some(),
            FilterMenu::Roles => !self.roles.is_empty(),
            FilterMenu::Signals => self.signals(),
        }
    }

    /// Module 636496's query from these filters: the safety signals into `or_query`, roles, the join date and the
    /// account age (a range of the user id, merged into the id search) into `and_query`.
    pub(super) fn apply(
        &self,
        and_query: &mut Map<String, Value>,
        or_query: &mut Map<String, Value>,
        now_ms: i64,
    ) {
        let mut signals = Map::new();
        if self.unusual_dm {
            signals.insert(
                "unusual_dm_activity_until".into(),
                json!({"range": {"gte": now_ms - UNUSUAL_DM_MS}}),
            );
        }
        if self.timed_out {
            signals.insert(
                "communication_disabled_until".into(),
                json!({"range": {"gte": now_ms}}),
            );
        }
        if self.unusual_account {
            signals.insert("unusual_account_activity".into(), json!(true));
        }
        if self.quarantined {
            signals.insert("automod_quarantined_username".into(), json!(true));
        }
        if !signals.is_empty() {
            or_query.insert("safety_signals".into(), Value::Object(signals));
        }
        if !self.roles.is_empty() {
            let ids: Vec<String> = self.roles.iter().map(Id::to_string).collect();
            and_query.insert("role_ids".into(), json!({"and_query": ids}));
        }
        if let Some(after) = self.joined.after {
            let mut range = Map::new();
            range.insert("gte".into(), json!(after));
            if let Some(before) = self.joined.before {
                range.insert("lte".into(), json!(before));
            }
            and_query.insert("guild_joined_at".into(), json!({"range": range}));
        }
        if let Some(after) = self.account.after {
            let mut range = Map::new();
            range.insert("gte".into(), json!(snowflake_at(after)));
            if let Some(before) = self.account.before {
                range.insert("lte".into(), json!(snowflake_at(before)));
            }
            let mut user = and_query
                .remove("user_id")
                .and_then(|value| value.as_object().cloned())
                .unwrap_or_default();
            user.insert("range".into(), Value::Object(range));
            and_query.insert("user_id".into(), Value::Object(user));
        }
    }
}

/// `fromTimestamp` of module 811091: the smallest snowflake of a moment, as a string.
fn snowflake_at(ms: i64) -> String {
    let since = ms - DISCORD_EPOCH_MS;
    if since <= 0 {
        "0".to_owned()
    } else {
        ((since as u64) << 22).to_string()
    }
}

/// `moment().subtract(input, unit).valueOf()`: hours by the clock, days, weeks and months by the calendar.
fn preset_after(now: DateTime<Local>, input: u32, unit: char) -> Option<i64> {
    let back = match unit {
        'h' => Some(now - chrono::Duration::hours(i64::from(input))),
        'd' => now.checked_sub_days(chrono::Days::new(u64::from(input))),
        'w' => now.checked_sub_days(chrono::Days::new(7 * u64::from(input))),
        'M' => now.checked_sub_months(Months::new(input)),
        _ => None,
    }?;
    Some(back.timestamp_millis())
}

/// `X` of module 920360, moment's `MMM Do, YYYY`: "Nov 24th, 2026".
fn date_label(ms: i64) -> Option<String> {
    let at = Local.timestamp_millis_opt(ms).single()?;
    let day = at.day();
    let suffix = match (day % 10, day % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    Some(format!("{} {day}{suffix}, {}", at.format("%b"), at.year()))
}

/// `j` of `el`: the Custom Range row's subtext while the range is picked.
fn custom_subtext(filter: &DateFilter) -> Option<String> {
    if !filter.custom() {
        return None;
    }
    let start = filter.after.and_then(date_label);
    let end = filter.before.and_then(date_label);
    match (start, end) {
        (Some(start), Some(end)) => Some(format!("{start} - {end}")),
        (Some(start), None) => Some(dc_intl::format("ClmSzd", &[("date", start.into())])),
        (None, Some(end)) => Some(dc_intl::format("YvNhsd", &[("date", end.into())])),
        (None, None) => None,
    }
}

/// What react-datepicker (module 7467 `setSelected`) hands `onChange` for a clicked day: the day at the time of day of
/// `selected` (module 939538 passes the bound, or now without one); the selected day itself changes nothing.
fn picked_at(day: NaiveDate, selected: DateTime<Local>) -> Option<i64> {
    if day == selected.date_naive() {
        return None;
    }
    let at = day.and_time(selected.time().with_nanosecond(0)?);
    Some(
        Local
            .from_local_datetime(&at)
            .earliest()?
            .timestamp_millis(),
    )
}

/// Module 91871: every character of `needle` in `hay`, in order.
fn fuzzy(needle: &str, hay: &str) -> bool {
    let mut rest = hay.chars();
    needle.chars().all(|want| rest.any(|have| have == want))
}

/// The open menu and what it shows.
pub(super) struct MenuState {
    pub(super) menu: FilterMenu,
    /// `x` of `el`: on the Custom Range view.
    custom: bool,
    /// The calendar open beside After Date (`true`) or Before Date, on the month it shows.
    calendar: Option<(bool, NaiveDate)>,
    role_search: Entity<Editor>,
    role_query: String,
    _role_events: Subscription,
}

/// A row of these menus (the join menu's `join_row`): 8px in, 14/18 medium `text-strong`, the hover tint.
fn menu_row(id: SharedString) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .min_h(px(36.))
        .p(px(8.))
        .rounded(px(2.))
        .cursor_pointer()
        .hover(|style| {
            style
                .bg(color("--interactive-background-hover"))
                .rounded(px(4.))
        })
        .flex()
        .items_center()
        .justify_between()
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-strong"))
}

impl Root {
    /// `eM` of module 920360 for a column with a menu: the 14/18 medium label (`text-strong` while filtered,
    /// `text-brand` while the table is sorted by it) and the 16px filter icon, brand while filtered. The dates need
    /// Manage Server; without it they are the bare label.
    pub(super) fn members_filter_header(
        &self,
        guild: Id,
        menu: FilterMenu,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.members_state() else {
            return div().into_any_element();
        };
        let filtered = state.filters.filtered(menu);
        let sorted = match menu {
            FilterMenu::Joined => state.sort == 2,
            FilterMenu::Account => matches!(state.sort, 3 | 4),
            _ => false,
        };
        let text = div()
            .min_w(px(0.))
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color(if sorted {
                "--text-brand"
            } else if filtered {
                "--text-strong"
            } else {
                "--text-default"
            }))
            .child(dc_intl::t(label));
        let needs_manage = matches!(menu, FilterMenu::Joined | FilterMenu::Account);
        if needs_manage && !self.my_permission(guild, None, MANAGE_GUILD) {
            return div().flex().child(text).into_any_element();
        }
        let header = div()
            .id(menu.anchor())
            .relative()
            .child(self.anchor_probe(menu.anchor()))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(text)
            .child(
                div().size(px(16.)).flex_none().pl(px(2.)).child(
                    icon(
                        "member_filter",
                        16.,
                        if filtered {
                            "--control-brand-foreground"
                        } else {
                            "--text-muted"
                        },
                    )
                    .with_transformation(gpui::Transformation::rotate(gpui::percentage(0.25))),
                ),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_filter_menu(guild, menu, cx)));
        if menu == FilterMenu::Signals {
            // The header's tooltip (`Z.m`, top): what the flags are.
            return self
                .with_tip(
                    "server-members-signals-tip",
                    super::tooltip::Tip::text(dc_intl::t("2cRO3R"), super::tooltip::Side::Top),
                    header,
                    cx,
                )
                .into_any_element();
        }
        header.into_any_element()
    }

    /// A header press: its menu opens on the main view, or closes when it was the open one.
    fn toggle_filter_menu(&mut self, guild: Id, menu: FilterMenu, cx: &mut Context<Self>) {
        let open = self
            .members_state()
            .and_then(|state| state.filter_menu.as_ref())
            .map(|open| open.menu);
        if open == Some(menu) {
            self.close_filter_menu();
            cx.notify();
            return;
        }
        let role_search = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder(dc_intl::t("ZveC7e").to_owned());
            editor
        });
        let events = cx.subscribe(
            &role_search,
            move |this, editor, event: &EditorEvent, cx| {
                match event {
                    EditorEvent::Edited => {
                        let text = editor.read(cx).text().to_owned();
                        if let Some(open) = this
                            .members_state_mut()
                            .and_then(|state| state.filter_menu.as_mut())
                        {
                            open.role_query = text;
                        }
                    }
                    EditorEvent::Cancel => this.close_filter_menu(),
                    EditorEvent::Submit(_) => {}
                }
                cx.notify();
            },
        );
        let _ = guild;
        if let Some(state) = self.members_state_mut() {
            state.join_open = false;
            state.filter_menu = Some(MenuState {
                menu,
                custom: false,
                calendar: None,
                role_search,
                role_query: String::new(),
                _role_events: events,
            });
        }
        cx.notify();
    }

    pub(super) fn close_filter_menu(&mut self) {
        if let Some(state) = self.members_state_mut() {
            state.filter_menu = None;
        }
    }

    /// The filters changed: module 406318's `MEMBER_SAFETY_SEARCH_STATE_UPDATE` searches again at once.
    fn set_filters(
        &mut self,
        guild: Id,
        change: impl FnOnce(&mut Filters),
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.members_state_mut() else {
            return;
        };
        change(&mut state.filters);
        self.research_members(guild, cx);
        cx.notify();
    }

    /// `onSelectDateOption`: the option with its after date from now, no before date.
    fn pick_date_option(
        &mut self,
        guild: Id,
        menu: FilterMenu,
        option: u8,
        cx: &mut Context<Self>,
    ) {
        let after = PRESETS
            .iter()
            .find(|(id, _, _)| *id == option)
            .and_then(|(_, back, _)| *back)
            .and_then(|(input, unit)| preset_after(Local::now(), input, unit));
        self.set_filters(
            guild,
            |filters| {
                *filters.date_mut(menu) = DateFilter {
                    option,
                    after,
                    before: None,
                }
            },
            cx,
        );
    }

    /// `p` / `onToggleCustomDateRange`: the Custom Range view, keeping a custom range already picked.
    fn open_custom_range(&mut self, guild: Id, menu: FilterMenu, cx: &mut Context<Self>) {
        if let Some(open) = self
            .members_state_mut()
            .and_then(|state| state.filter_menu.as_mut())
        {
            open.custom = true;
        }
        self.set_filters(
            guild,
            |filters| {
                let date = filters.date_mut(menu);
                let custom = date.custom();
                *date = DateFilter {
                    option: CUSTOM,
                    after: date.after.filter(|_| custom),
                    before: date.before.filter(|_| custom),
                };
            },
            cx,
        );
    }

    /// `A`: back to the presets; a range with neither date goes back to All.
    fn leave_custom_range(&mut self, guild: Id, menu: FilterMenu, cx: &mut Context<Self>) {
        if let Some(open) = self
            .members_state_mut()
            .and_then(|state| state.filter_menu.as_mut())
        {
            open.custom = false;
            open.calendar = None;
        }
        let empty = self.members_state().is_some_and(|state| {
            let date = state.filters.date(menu);
            date.after.is_none() && date.before.is_none()
        });
        if empty {
            self.pick_date_option(guild, menu, 0, cx);
        }
        cx.notify();
    }

    /// `onSelectStartDate` / `onSelectEndDate`: the picked day as the bound (see [`picked_at`]).
    fn pick_custom_date(
        &mut self,
        guild: Id,
        menu: FilterMenu,
        after: bool,
        day: NaiveDate,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.members_state() else {
            return;
        };
        let date = *state.filters.date(menu);
        let bound = if after { date.after } else { date.before };
        let selected = bound
            .filter(|_| date.custom())
            .and_then(|ms| Local.timestamp_millis_opt(ms).single())
            .unwrap_or_else(Local::now);
        let Some(ms) = picked_at(day, selected) else {
            return;
        };
        self.set_filters(
            guild,
            |filters| {
                let date = filters.date_mut(menu);
                date.option = CUSTOM;
                if after {
                    date.after = Some(ms);
                } else {
                    date.before = Some(ms);
                }
            },
            cx,
        );
    }

    /// The open column menu, under its header (bottom, left aligned, 4px below), as the join menu is drawn.
    pub(super) fn members_filter_layer(
        &self,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.members_state()?;
        let open = state.filter_menu.as_ref()?;
        let guild = self.members_guild()?;
        let menu = open.menu;
        let anchor = self.tip_anchors.borrow().get(menu.anchor()).copied()?;
        let (id, label, rows): (&'static str, &'static str, Vec<AnyElement>) = match menu {
            FilterMenu::Joined | FilterMenu::Account => (
                if menu == FilterMenu::Joined {
                    "member-safety-guild-member-joined-date-menu"
                } else {
                    "member-safety-guild-member-account-age-menu"
                },
                if menu == FilterMenu::Joined {
                    "XMVinX"
                } else {
                    "D++Tgf"
                },
                self.date_menu_rows(guild, menu, open, &state.filters, cx),
            ),
            FilterMenu::Roles => (
                "member-safety-roles",
                "ZveC7e",
                self.role_menu_rows(guild, open, &state.filters, cx),
            ),
            FilterMenu::Signals => (
                "member-safety-flags",
                "k9m8Rg",
                self.signal_menu_rows(guild, &state.filters, cx),
            ),
        };
        let calendar = open.calendar.and_then(|(after, month)| {
            self.filter_calendar(guild, menu, after, month, &state.filters, cx)
        });
        let header = menu.anchor();
        let menu_box = self.tip_anchors.borrow().get(MENU_ANCHOR).copied();
        // The After Date and Before Date items' submenu (`submenu_M3qk4j calendarContainer_K-iqS8`) beside their row:
        // one item rendering the picker, `calendar_K-iqS8` moving it 2px down without growing the submenu.
        let row = open
            .calendar
            .and_then(|(after, _)| self.tip_anchors.borrow().get(date_row(after)).copied());
        let calendar = calendar.zip(row).map(|(calendar, row)| {
            let item = div()
                .mt(px(2.))
                .mb(px(-2.))
                .child(calendar)
                .into_any_element();
            self.submenu_frame_wide(
                vec![(item, CALENDAR_HEIGHT)],
                row,
                window,
                CALENDAR_WIDTH + 18.,
            )
        });
        // `nudgeLeftAlignment` of module 623646 (on by default in 265872): a left-aligned layer moves left to keep 12px
        // from the window's right edge, and never closer than 12px to its left.
        let width = menu_box.map_or(px(188.), |bounds| bounds.size.width);
        let left = anchor
            .left()
            .min(window.viewport_size().width - width - px(12.))
            .max(px(12.));
        let menu_element = gpui::deferred(
            div()
                .id(id)
                .aria_label(dc_intl::t(label))
                .popout(id, PopKind::Fade, PopSide::Bottom)
                .occlude()
                .absolute()
                .left(left)
                .top(anchor.bottom() + px(4.))
                // `menu_M3qk4j flexible_M3qk4j`: 188 to 320 wide.
                .min_w(px(188.))
                .max_w(px(320.))
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: gpui::point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                // GAP: `--reference-position-layer-max-height` is not measured; as the join menu, 16px short of the
                // window's bottom, the rows scrolling.
                .max_h(px((f32::from(window.viewport_size().height)
                    - f32::from(anchor.bottom())
                    - 20.)
                    .max(120.)))
                .flex()
                .flex_col()
                .on_mouse_down_out(
                    cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                        let inside = |key: &str| {
                            this.tip_anchors
                                .borrow()
                                .get(key)
                                .is_some_and(|bounds| bounds.contains(&event.position))
                        };
                        // A press on the header is its own toggle; one on the calendar picks a day.
                        let calendar_open = this
                            .members_state()
                            .and_then(|state| state.filter_menu.as_ref())
                            .is_some_and(|open| open.calendar.is_some());
                        if !inside(header) && !(calendar_open && inside(CALENDAR_ANCHOR)) {
                            this.close_filter_menu();
                            cx.notify();
                        }
                    }),
                )
                .child(self.anchor_probe(MENU_ANCHOR))
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-scroll")))
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .children(rows),
                ),
        )
        // Over Server Settings (drawn at 1), as the join menu.
        .with_priority(2)
        .into_any_element();
        Some(
            div()
                .child(menu_element)
                .children(calendar)
                .into_any_element(),
        )
    }

    /// `el`: the presets, a separator and Custom Range; or the Custom Range view.
    fn date_menu_rows(
        &self,
        guild: Id,
        menu: FilterMenu,
        open: &MenuState,
        filters: &Filters,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let date = *filters.date(menu);
        let name = menu.date_name();
        let mut rows = Vec::new();
        if open.custom {
            // `nagivateBackContainer_K-iqS8`: 6px 8px 6px 7px, 2px apart, the brand hover.
            rows.push(
                div()
                    .id("back")
                    .flex_none()
                    .my(px(2.))
                    .py(px(6.))
                    .pl(px(7.))
                    .pr(px(8.))
                    .rounded(px(2.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_color(color("--text-strong"))
                    .hover(|style| style.bg(color("--brand-560")).text_color(color("--white")))
                    .child(icon("chevron_small_left", 16., "--text-strong"))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .child(dc_intl::t("BTfN6g")),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.leave_custom_range(guild, menu, cx)),
                    )
                    .into_any_element(),
            );
            for (after, key, id) in [
                (true, "RDqVOD", "after-date-menu-item"),
                (false, "jF54hQ", "before-date-menu-item"),
            ] {
                let bound = if after { date.after } else { date.before };
                let subtext = bound.filter(|_| date.custom()).and_then(date_label);
                let shown = open.calendar.map(|(which, _)| which) == Some(after);
                rows.push(
                    menu_row(id.into())
                        .relative()
                        .child(self.anchor_probe(date_row(after)))
                        .when(shown, |row| {
                            row.bg(color("--interactive-background-hover"))
                                .rounded(px(4.))
                        })
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .child(div().whitespace_nowrap().child(dc_intl::t(key)))
                                .children(subtext.map(|text| {
                                    // `subtext_M3qk4j`: 2px under the label, `text-muted`.
                                    div()
                                        .mt(px(2.))
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(400.))
                                        .text_color(color("--text-muted"))
                                        .child(text)
                                })),
                        )
                        .child(icon("menu_caret", 20., "--text-muted").ml(px(8.)))
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered {
                                this.show_filter_calendar(menu, after, cx);
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.show_filter_calendar(menu, after, cx)
                        }))
                        .into_any_element(),
                );
            }
            return rows;
        }
        for (option, _, key) in PRESETS {
            rows.push(
                menu_row(format!("guild-member-{name}-option-{option}").into())
                    .child(div().flex_1().whitespace_nowrap().child(dc_intl::t(key)))
                    .child(super::create_channel::radio(date.option == option).ml(px(8.)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pick_date_option(guild, menu, option, cx)
                    }))
                    .into_any_element(),
            );
        }
        rows.push(
            div()
                .my(px(8.))
                .h(px(1.))
                .flex_none()
                .bg(color("--border-subtle"))
                .into_any_element(),
        );
        // `otherDatesContainer_K-iqS8`: 6px 8px, 2px apart, the label column 4px apart; the picked range's radio
        // (`radioSelection_K-iqS8`, 18px) or the chevron.
        let subtext = custom_subtext(&date);
        rows.push(
            div()
                .id(SharedString::from(format!(
                    "guild-member-{name}-custom-option"
                )))
                .group("custom-range")
                .flex_none()
                .my(px(2.))
                .py(px(6.))
                .px(px(8.))
                .rounded(px(2.))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .hover(|style| style.bg(color("--brand-600")))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--interactive-text-default"))
                                .group_hover("custom-range", |style| {
                                    style.text_color(color("--white"))
                                })
                                .child(dc_intl::t("BTfN6g")),
                        )
                        .children(subtext.map(|text| {
                            // `footnote/medium` in `text-muted`.
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-muted"))
                                .group_hover("custom-range", |style| {
                                    style.text_color(color("--white"))
                                })
                                .child(text)
                        })),
                )
                .child(if date.custom() {
                    // Module 282054 at 18px: the ring in the row's colour, the 5-of-24 dot in the brand foreground.
                    div()
                        .relative()
                        .size(px(18.))
                        .flex_none()
                        .child(icon("member_radio_ring", 18., "--interactive-text-default"))
                        .child(
                            div()
                                .absolute()
                                .left(px(5.25))
                                .top(px(5.25))
                                .size(px(7.5))
                                .rounded_full()
                                .bg(color("--control-brand-foreground")),
                        )
                        .into_any_element()
                } else {
                    icon("bot_chevron_small_right", 16., "--interactive-text-default")
                        .group_hover("custom-range", |style| style.text_color(color("--white")))
                        .into_any_element()
                })
                .on_click(
                    cx.listener(move |this, _, _, cx| this.open_custom_range(guild, menu, cx)),
                )
                .into_any_element(),
        );
        rows
    }

    /// Hovering After Date or Before Date opens its calendar on the picked day's month, or this month.
    fn show_filter_calendar(&mut self, menu: FilterMenu, after: bool, cx: &mut Context<Self>) {
        let Some(state) = self.members_state_mut() else {
            return;
        };
        let date = *state.filters.date(menu);
        let Some(open) = state.filter_menu.as_mut() else {
            return;
        };
        if open.calendar.map(|(which, _)| which) == Some(after) {
            return;
        }
        let bound = if after { date.after } else { date.before };
        let day = bound
            .filter(|_| date.custom())
            .and_then(|ms| Local.timestamp_millis_opt(ms).single())
            .map_or_else(|| Local::now().date_naive(), |at| at.date_naive());
        open.calendar = Some((after, day.with_day(1).unwrap_or(day)));
        cx.notify();
    }

    /// Module 939538 beside the menu: today at most, back to May 15th 2015 (`J`), and not before the after date for
    /// the before date.
    fn filter_calendar(
        &self,
        guild: Id,
        menu: FilterMenu,
        after: bool,
        month: NaiveDate,
        filters: &Filters,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let date = *filters.date(menu);
        let day = |ms: Option<i64>| {
            ms.filter(|_| date.custom())
                .and_then(|ms| Local.timestamp_millis_opt(ms).single())
                .map(|at| at.date_naive())
        };
        let today = Local::now().date_naive();
        let first = NaiveDate::from_ymd_opt(2015, 5, 15)?;
        let min = if after {
            first
        } else {
            day(date.after).unwrap_or(first)
        };
        let selected = day(if after { date.after } else { date.before }).unwrap_or(today);
        // GAP: the submenu's own frame around the calendar is not captured; the picker sits 8px right of the menu.
        Some(super::guild_events::pickers::calendar(
            month,
            selected,
            min,
            today,
            move |this, day, _, cx| this.pick_custom_date(guild, menu, after, day, cx),
            |this, step, _, _| {
                if let Some(open) = this
                    .members_state_mut()
                    .and_then(|state| state.filter_menu.as_mut())
                    && let Some((which, month)) = open.calendar
                {
                    let months = month.year() * 12 + month.month0() as i32 + step;
                    let (year, number) = (months.div_euclid(12), months.rem_euclid(12) as u32 + 1);
                    open.calendar = Some((
                        which,
                        NaiveDate::from_ymd_opt(year, number, 1).unwrap_or(month),
                    ));
                }
            },
            cx,
        ))
    }

    /// `eV`: the role box, a separator, then a check row with its dot for each role that matches.
    fn role_menu_rows(
        &self,
        guild: Id,
        open: &MenuState,
        filters: &Filters,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut rows = vec![
            div()
                .id("members-table-role-search")
                .flex_none()
                .h(px(32.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(color("--input-background-default"))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(icon("picker_search", 14., "--input-text-default"))
                .child(div().flex_1().min_w(px(0.)).child(open.role_search.clone()))
                .into_any_element(),
            div()
                .my(px(8.))
                .h(px(1.))
                .flex_none()
                .bg(color("--border-subtle"))
                .into_any_element(),
        ];
        let Some(known) = self.stores.guild(guild) else {
            return rows;
        };
        let fallback = {
            let rgba = color("--primary-300");
            let byte = |part: f32| (part.clamp(0., 1.) * 255.).round() as u32;
            (byte(rgba.r) << 16) | (byte(rgba.g) << 8) | byte(rgba.b)
        };
        let query = open.role_query.to_lowercase();
        let dot_style = self.pref_index("role_style", 0) == 1;
        let mut roles: Vec<(i64, Id, &String)> = known
            .roles
            .iter()
            .filter(|(id, _)| **id != guild)
            .map(|(id, name)| {
                let position = known.role_info.get(id).map_or(0, |info| info.position);
                (position, *id, name)
            })
            .collect();
        // `AT` of module 7864: by position, highest first, then by id.
        roles.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for (_, role, name) in roles {
            if !query.is_empty() && !fuzzy(&query, &name.to_lowercase()) {
                continue;
            }
            let info = known.role_info.get(&role).copied().unwrap_or_default();
            let tint = if info.color == 0 {
                fallback
            } else {
                info.color
            };
            // `roleDot` of module 165862: the dot (`W` of 545442) with the Role Colors dot style, else the circle
            // (`R`); the gradient where the server has `ENHANCED_ROLE_COLORS` (`jV` of module 676608).
            let dot = match (dot_style, known.role_colors_of(role)) {
                (true, Some(colors)) => super::role_dot::dot_gradient(&colors),
                (true, None) => super::role_dot::dot(tint),
                (false, Some(colors)) => div()
                    .size(px(12.))
                    .child(gpui::img(super::role_gradient::circle(&colors, 12, 1.)).size(px(12.))),
                (false, None) => div().size(px(12.)).rounded_full().bg(gpui::rgb(tint)),
            };
            let checked = filters.roles.contains(&role);
            rows.push(
                menu_row(format!("role-{role}").into())
                    // `roleDotAccessory_M3qk4j`: a 20px box 8px before the label.
                    .child(
                        div()
                            .size(px(20.))
                            .mr(px(8.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(dot),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(name.clone()),
                    )
                    .child(super::context_menu::menu_check_box(checked))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_filters(
                            guild,
                            |filters| {
                                if let Some(at) = filters.roles.iter().position(|id| *id == role) {
                                    filters.roles.remove(at);
                                } else {
                                    filters.roles.push(role);
                                }
                            },
                            cx,
                        )
                    }))
                    .into_any_element(),
            );
        }
        rows
    }

    /// `ec`: the four flags, each a check row with its icon 8px before the label.
    fn signal_menu_rows(
        &self,
        guild: Id,
        filters: &Filters,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        type Flag = fn(&mut Filters) -> &mut bool;
        let flags: [(&str, &str, &str, bool, Flag); 4] = [
            (
                "toggle-require-unusual-dm-activity",
                "ZRnON3",
                "mod_info_chat_warning",
                filters.unusual_dm,
                |filters| &mut filters.unusual_dm,
            ),
            (
                "toggle-require-communication-disabled",
                "z3wbj8",
                "mod_info_clock_warning",
                filters.timed_out,
                |filters| &mut filters.timed_out,
            ),
            (
                "toggle-require-unusual-account-activity",
                "DIQsD9",
                "member_unusual_account",
                filters.unusual_account,
                |filters| &mut filters.unusual_account,
            ),
            (
                "toggle-require-username-quarantined",
                "Jloklk",
                "member_chat_x",
                filters.quarantined,
                |filters| &mut filters.quarantined,
            ),
        ];
        flags
            .into_iter()
            .map(|(id, key, glyph, checked, flag)| {
                menu_row(id.into())
                    // `iconContainerLeft_M3qk4j`: a 20px icon 8px before the label.
                    .child(icon(glyph, 20., "--interactive-text-default").mr(px(8.)))
                    .child(div().flex_1().whitespace_nowrap().child(dc_intl::t(key)))
                    .child(super::context_menu::menu_check_box(checked))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_filters(
                            guild,
                            |filters| {
                                let on = flag(filters);
                                *on = !*on;
                            },
                            cx,
                        )
                    }))
                    .into_any_element()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Module 636496: flags into `or_query.safety_signals`, roles as an `and_query`, the join date as a range of
    /// times and the account age as a range of snowflakes merged with the id search.
    #[test]
    fn filters_shape_the_search_as_module_636496() {
        let filters = Filters {
            joined: DateFilter {
                option: CUSTOM,
                after: Some(1_700_000_000_000),
                before: Some(1_700_086_400_000),
            },
            account: DateFilter {
                option: 3,
                after: Some(DISCORD_EPOCH_MS + 1),
                before: None,
            },
            roles: vec![5, 3],
            unusual_dm: true,
            timed_out: true,
            unusual_account: false,
            quarantined: true,
        };
        let mut and_query = Map::new();
        and_query.insert("user_id".into(), json!({"or_query": ["9"]}));
        let mut or_query = Map::new();
        filters.apply(&mut and_query, &mut or_query, 10_000_000_000);
        assert_eq!(
            Value::Object(or_query),
            json!({"safety_signals": {
                "unusual_dm_activity_until": {"range": {"gte": 10_000_000_000i64 - 172_800_000}},
                "communication_disabled_until": {"range": {"gte": 10_000_000_000i64}},
                "automod_quarantined_username": true,
            }})
        );
        assert_eq!(
            Value::Object(and_query),
            json!({
                "role_ids": {"and_query": ["5", "3"]},
                "guild_joined_at": {"range": {"gte": 1_700_000_000_000i64, "lte": 1_700_086_400_000i64}},
                "user_id": {"or_query": ["9"], "range": {"gte": (1u64 << 22).to_string()}},
            })
        );
    }

    /// A date only filters from its after date; with neither the query is untouched.
    #[test]
    fn a_range_without_an_after_date_does_not_filter() {
        let filters = Filters {
            joined: DateFilter {
                option: CUSTOM,
                after: None,
                before: Some(5),
            },
            ..Filters::default()
        };
        assert!(!filters.filtered(FilterMenu::Joined));
        let (mut and_query, mut or_query) = (Map::new(), Map::new());
        filters.apply(&mut and_query, &mut or_query, 0);
        assert!(and_query.is_empty() && or_query.is_empty());
    }

    #[test]
    fn labels_follow_moment() {
        let at = |y, m, d| {
            Local
                .with_ymd_and_hms(y, m, d, 12, 0, 0)
                .single()
                .map(|at| at.timestamp_millis())
        };
        assert_eq!(
            date_label(at(2026, 11, 1).unwrap()).as_deref(),
            Some("Nov 1st, 2026")
        );
        assert_eq!(
            date_label(at(2026, 11, 12).unwrap()).as_deref(),
            Some("Nov 12th, 2026")
        );
        assert_eq!(
            date_label(at(2026, 5, 23).unwrap()).as_deref(),
            Some("May 23rd, 2026")
        );
        let range = DateFilter {
            option: CUSTOM,
            after: at(2026, 11, 1),
            before: at(2026, 11, 22),
        };
        assert_eq!(
            custom_subtext(&range).as_deref(),
            Some("Nov 1st, 2026 - Nov 22nd, 2026")
        );
        let preset = DateFilter { option: 3, ..range };
        assert_eq!(custom_subtext(&preset), None);
    }

    #[test]
    fn presets_reach_back_by_the_calendar() {
        let now = Local
            .with_ymd_and_hms(2026, 3, 31, 10, 0, 0)
            .single()
            .unwrap();
        let back = |input, unit| {
            Local
                .timestamp_millis_opt(preset_after(now, input, unit).unwrap())
                .single()
                .unwrap()
        };
        assert_eq!(back(1, 'h').hour(), 9);
        assert_eq!(
            back(2, 'w').date_naive(),
            NaiveDate::from_ymd_opt(2026, 3, 17).unwrap()
        );
        assert_eq!(
            back(3, 'M').date_naive(),
            NaiveDate::from_ymd_opt(2025, 12, 31).unwrap()
        );
    }

    /// Module 7467 keeps the selected time of day on the clicked day, and ignores a click on the selected day.
    #[test]
    fn a_picked_day_keeps_the_selected_time() {
        let selected = Local
            .with_ymd_and_hms(2026, 11, 24, 15, 30, 12)
            .single()
            .unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let want = Local
            .with_ymd_and_hms(2026, 11, 9, 15, 30, 12)
            .single()
            .unwrap();
        assert_eq!(picked_at(day, selected), Some(want.timestamp_millis()));
        assert_eq!(picked_at(selected.date_naive(), selected), None);
    }

    #[test]
    fn role_search_matches_in_order() {
        assert!(fuzzy("grd", "gravecord red"));
        assert!(!fuzzy("dg", "gravecord red"));
        assert!(fuzzy("", "anything"));
    }
}
