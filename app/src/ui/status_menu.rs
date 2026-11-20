//! The account popout of the user panel (`accountPopout_g6VwXp`): the profile card with the custom status bubble, the
//! Edit Profile / status / Badges / Orbs rows and Switch Accounts; the status menu hanging off the status row
//! (`statusPickerModalMenu_nitlyF`) with its durations; and the "Set your status" modal (module 431273).
//! Measured in `design/dom/popouts/profile-status-darker-cozy.json`, `profile-status-durations-darker-cozy.json`
//! and `design/dom/modals/custom-status-darker-cozy.json`.
//!
//! A choice is written to the account's proto settings (`status`, message 11): the whole message goes in the PATCH
//! (`dc_proto::settings::set_status`), as `updateStatus` in module 827827 does.

use super::modal::{self, Tone};
use super::motion::{PopKind, PopSide, Popout as _};
use super::{Root, avatar_source, color, icon, status_dot_large};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_proto::settings::CustomStatus;
use gpui::{
    AnyElement, BoxShadow, Context, Entity, Focusable, FontWeight, IntoElement, MouseButton,
    SharedString, Subscription, Window, anchored, deferred, div, img, point, prelude::*, px,
};
use serde_json::json;

pub(super) const HOUR: u64 = 3_600_000;
const MINUTE: u64 = 60_000;
/// "For 15 Minutes" ... "Forever": the durations of the status menu, in milliseconds (`None`: until changed).
pub(super) const DURATIONS: [(&str, Option<u64>); 6] = [
    ("For 15 Minutes", Some(15 * MINUTE)),
    ("For 1 Hour", Some(HOUR)),
    ("For 8 Hours", Some(8 * HOUR)),
    ("For 24 Hours", Some(24 * HOUR)),
    ("For 3 Days", Some(72 * HOUR)),
    ("Forever", None),
];

/// The statuses of the menu: key, label and the line under it.
const STATUSES: [(&str, &str, Option<&str>); 4] = [
    ("online", "Online", None),
    ("idle", "Idle", None),
    (
        "dnd",
        "Do Not Disturb",
        Some("You will not receive desktop notifications"),
    ),
    ("invisible", "Invisible", Some("You will appear offline")),
];

/// When a status chosen at `now_ms` for `duration` lapses; zero for never.
pub(super) fn expires_at(now_ms: u64, duration: Option<u64>) -> u64 {
    duration.map_or(0, |duration| now_ms + duration)
}

/// What the custom status modal clears the status after (`$.SX` in module 403918, in the order of its list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClearAfter {
    Today,
    Hours4,
    Hour1,
    Minutes30,
    Never,
}

impl ClearAfter {
    const ALL: [ClearAfter; 5] = [
        ClearAfter::Today,
        ClearAfter::Hours4,
        ClearAfter::Hour1,
        ClearAfter::Minutes30,
        ClearAfter::Never,
    ];

    /// `z` in module 431273: Today is a day from now.
    fn millis(self) -> Option<u64> {
        match self {
            ClearAfter::Today => Some(24 * HOUR),
            ClearAfter::Hours4 => Some(4 * HOUR),
            ClearAfter::Hour1 => Some(HOUR),
            ClearAfter::Minutes30 => Some(30 * MINUTE),
            ClearAfter::Never => None,
        }
    }

    /// The choice that matches a status already set to lapse at `expires_ms` (`useState` in module 431273).
    pub(super) fn from_expiry(expires_ms: u64, now_ms: u64, same_day: bool) -> Self {
        if expires_ms == 0 {
            return ClearAfter::Never;
        }
        if !same_day {
            return ClearAfter::Today;
        }
        let left = expires_ms.saturating_sub(now_ms);
        [ClearAfter::Minutes30, ClearAfter::Hour1, ClearAfter::Hours4]
            .into_iter()
            .find(|choice| choice.millis().is_some_and(|millis| left <= millis))
            .unwrap_or(ClearAfter::Today)
    }
}

/// The popout and what hangs off it.
#[derive(Default)]
pub(super) struct StatusPopout {
    /// Which submenu is open: the status list, or the durations of one of its statuses.
    pub submenu: Submenu,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Submenu {
    #[default]
    None,
    Statuses,
    /// The durations of this status (index into `STATUSES`).
    Durations(usize),
    Accounts,
}

pub(super) struct CustomStatusModal {
    pub input: Entity<Editor>,
    pub clear: ClearAfter,
    pub clear_open: bool,
    _events: Subscription,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

/// The clock reading of a moment `after` milliseconds from now, as the clear-after labels write it.
fn clear_time(after: u64) -> (String, bool) {
    let then = chrono::Local::now() + chrono::Duration::milliseconds(after as i64);
    let same_day = then.date_naive() == chrono::Local::now().date_naive();
    (super::settings::prefs::clock(&then), same_day)
}

/// `V` in module 431273: the label of the dropdown's trigger.
fn clear_label(choice: ClearAfter) -> String {
    match choice.millis() {
        None => "Don't clear".to_owned(),
        Some(after) => {
            let (time, same_day) = clear_time(after);
            if same_day {
                format!("Clear at {time}")
            } else {
                format!("Clear tomorrow at {time}")
            }
        }
    }
}

/// The dropdown's rows: `24 hours (tomorrow at 8:48 AM)`.
fn clear_option(choice: ClearAfter) -> String {
    let (count, unit) = match choice {
        ClearAfter::Today => (24, "hours"),
        ClearAfter::Hours4 => (4, "hours"),
        ClearAfter::Hour1 => (1, "hour"),
        ClearAfter::Minutes30 => (30, "minutes"),
        ClearAfter::Never => return "Don't clear".to_owned(),
    };
    let (time, same_day) = clear_time(choice.millis().unwrap_or(0));
    let when = if same_day {
        time
    } else {
        format!("tomorrow at {time}")
    };
    format!("{count} {unit} ({when})")
}

fn menu_surface(cx_name: &str) -> gpui::Rgba {
    color(cx_name)
}

impl Root {
    pub(super) fn toggle_status_popout(&mut self, cx: &mut Context<Self>) {
        self.status_popout = match self.status_popout {
            Some(_) => None,
            None => Some(StatusPopout::default()),
        };
        cx.notify();
    }

    /// `updateStatus`: the status and when it lapses, written to the account.
    pub(super) fn choose_status(
        &mut self,
        status: &str,
        duration: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let now = now_ms();
        let patch = self
            .stores
            .set_status(status, expires_at(now, duration), now);
        self.patch_settings(patch);
        self.refresh_own_presence();
        self.ring_reconcile(cx);
        self.status_popout = None;
        cx.notify();
    }

    fn patch_settings(&mut self, patch: String) {
        // Headless runs set this so a scripted click can never change the account's visible status.
        #[cfg(feature = "capture")]
        if std::env::var_os("GRAVECORD_NO_SETTINGS_WRITE").is_some() {
            return;
        }
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: "/users/@me/settings-proto/1".to_owned(),
            body: Some(json!({"settings": patch})),
        });
    }

    pub(super) fn open_custom_status(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.stores.status_state();
        let current = state.as_ref().and_then(|state| state.custom.clone());
        let input = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Hit a high score in...".to_owned());
            if let Some(custom) = &current {
                editor.set_text(&custom.text);
            }
            editor
        });
        let events = cx.subscribe(&input, |this, input, event: &EditorEvent, cx| {
            match event {
                // Enter clears the field before it reports, so the text goes back before it is read.
                EditorEvent::Submit(text) => {
                    input.update(cx, |editor, _| editor.set_text(text));
                    this.save_custom_status(cx)
                }
                EditorEvent::Cancel => this.custom_status = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        window.focus(&input.read(cx).focus_handle(cx), cx);
        let clear = match &current {
            Some(custom) => {
                let now = now_ms();
                let same_day = custom.expires_at_ms != 0
                    && chrono::DateTime::from_timestamp_millis(custom.expires_at_ms as i64)
                        .is_some_and(|then| {
                            then.with_timezone(&chrono::Local).date_naive()
                                == chrono::Local::now().date_naive()
                        });
                ClearAfter::from_expiry(custom.expires_at_ms, now, same_day)
            }
            None => ClearAfter::Today,
        };
        self.custom_status = Some(CustomStatusModal {
            input,
            clear,
            clear_open: false,
            _events: events,
        });
        self.status_popout = None;
        cx.notify();
    }

    /// Save: the text and when to clear it; an empty text clears the custom status (`updateSetting(undefined)`).
    fn save_custom_status(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.custom_status.take() else {
            return;
        };
        let text = open.input.read(cx).text().trim().to_owned();
        let now = now_ms();
        let custom = (!text.is_empty()).then(|| CustomStatus {
            text,
            emoji_id: 0,
            emoji_name: String::new(),
            expires_at_ms: open.clear.millis().map_or(0, |after| now + after),
        });
        let patch = self.stores.set_custom_status(custom.as_ref(), now);
        self.patch_settings(patch);
        self.refresh_own_presence();
        cx.notify();
    }

    fn status_row_label(&self) -> &'static str {
        match self.stores.status.as_str() {
            "dnd" => "Do Not Disturb",
            "idle" => "Idle",
            "invisible" => "Invisible",
            _ => "Online",
        }
    }

    /// One row of the popout's menus: icon, label, and a chevron when it opens something.
    #[allow(clippy::too_many_arguments)]
    fn popout_row(
        &self,
        id: &'static str,
        leading: AnyElement,
        label: &'static str,
        trailing: Option<AnyElement>,
        open: bool,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
        hover: Submenu,
    ) -> AnyElement {
        div()
            .id(id)
            .h(px(32.))
            .px(px(8.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(if open {
                color("--text-strong")
            } else {
                color("--interactive-text-default")
            })
            .when(open, |this| {
                this.bg(color("--interactive-background-hover"))
            })
            .hover(|style| {
                style
                    .bg(color("--interactive-background-hover"))
                    .text_color(color("--text-strong"))
            })
            .child(div().size(px(16.)).flex_none().child(leading))
            .child(div().flex_1().child(label))
            .children(trailing)
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered && let Some(popout) = this.status_popout.as_mut() {
                    popout.submenu = hover;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                run(this, window, cx);
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    pub(super) fn status_popout_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let popout = self.status_popout.as_ref()?;
        let user = self.stores.current_user.clone().unwrap_or_default();
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let banner = self.banner_color(&user);
        let badges = self.badges(user.id);
        let state = self.stores.status_state();
        let custom_text = state
            .as_ref()
            .and_then(|state| state.custom.as_ref())
            .map(|custom| custom.text.clone());
        let status = self.stores.status.clone();
        let surface = color("--background-surface-high");
        let pencil = icon("pm_edit", 16., "--interactive-text-default").into_any_element();
        let chevron = || {
            div()
                .w(px(24.))
                .flex()
                .justify_end()
                .child(icon("pm_chevron", 16., "--interactive-icon-default"))
                .into_any_element()
        };
        let new_badge = div()
            .h(px(16.))
            .px(px(8.))
            .rounded(px(8.))
            .bg(color("--brand-500"))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(600.))
            .text_color(gpui::white())
            .child("NEW")
            .into_any_element();
        let status_icon =
            || icon(status_menu_icon(&status), 12., status_tint(&status)).into_any_element();
        let status_open = matches!(popout.submenu, Submenu::Statuses | Submenu::Durations(_));
        let dnd_bell = (status == "dnd")
            .then(|| icon("pm_dnd_bell", 12., "--interactive-icon-default").into_any_element());

        let header = div()
            .relative()
            .flex_none()
            .h(px(140.))
            .child(div().h(px(105.)).rounded_t(px(8.)).bg(banner))
            .child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(55.))
                    .size(px(92.))
                    .rounded_full()
                    .bg(surface)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        self.with_tip(
                            "status-popout-avatar-tip",
                            super::tooltip::Tip::text(
                                dc_intl::t!("+Xp3hq"),
                                super::tooltip::Side::Top,
                            ),
                            div()
                                .id("status-popout-avatar")
                                .group("status-popout-avatar")
                                .relative()
                                .size(px(80.))
                                .rounded_full()
                                .cursor_pointer()
                                .children(
                                    avatar.map(|picture| img(picture).size(px(80.)).rounded_full()),
                                )
                                .child(super::profile::popout_avatar_hover_overlay(
                                    "status-popout-avatar",
                                ))
                                .child(status_dot_large(&status, "--background-surface-high"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.status_popout = None;
                                    this.open_full_profile(user.id, window, cx);
                                })),
                            cx,
                        ),
                    ),
            )
            .child(self.status_bubble(custom_text, cx));

        let names = div()
            .h(px(48.))
            .flex()
            .flex_col()
            .child(
                div().h(px(24.)).flex().items_center().gap(px(4.)).child(
                    div()
                        .id("status-popout-display-name")
                        .cursor_pointer()
                        .hover(|style| style.underline())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.status_popout = None;
                            this.open_full_profile(user.id, window, cx);
                        }))
                        .text_size(px(20.))
                        .line_height(px(24.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(user.display_name().to_owned()),
                ),
            )
            .child(
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(9.33))
                    .child({
                        let (shown, concealed) = self.profile_username(&user);
                        let line = div()
                            .id("status-popout-username")
                            .cursor_pointer()
                            .hover(|style| style.underline())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.status_popout = None;
                                this.open_full_profile(user.id, window, cx);
                            }))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-strong"))
                            .child(shown);
                        // Module 297413: a concealed name carries "Streamer Mode Enabled".
                        if concealed {
                            self.with_tip(
                                "status-popout-username-streamer",
                                super::tooltip::Tip::text(
                                    dc_intl::t("Br1ls3"),
                                    super::tooltip::Side::Top,
                                ),
                                line,
                                cx,
                            )
                            .into_any_element()
                        } else {
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(line)
                                .child(self.copy_username_button(user.username.clone(), cx))
                                .into_any_element()
                        }
                    })
                    .when(!badges.is_empty(), |this| {
                        this.child(
                            div()
                                .h(px(22.))
                                .px(px(3.))
                                .py(px(1.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-muted"))
                                .bg(color("--background-mod-subtle"))
                                .flex()
                                .items_center()
                                .gap(px(2.))
                                .children(badges),
                        )
                    }),
            );

        let separator = || {
            div()
                .my(px(8.))
                .h(px(1.))
                .mx(px(8.))
                .bg(color("--border-subtle"))
        };
        let group = |rows: Vec<AnyElement>| {
            div()
                .p(px(8.))
                .rounded(px(8.))
                .bg(color("--background-surface-highest"))
                .flex()
                .flex_col()
                .children(rows)
        };
        let first = group(vec![
            self.popout_row(
                "status-edit-profile",
                pencil,
                "Edit Profile",
                Some(new_badge),
                false,
                cx,
                |this, _, cx| {
                    this.status_popout = None;
                    this.open_profile_editor(cx);
                },
                Submenu::None,
            ),
            separator().into_any_element(),
            self.popout_row(
                "status-row",
                status_icon(),
                self.status_row_label(),
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .children(dnd_bell)
                        .child(chevron())
                        .into_any_element(),
                ),
                status_open,
                cx,
                |this, _, cx| {
                    if let Some(popout) = this.status_popout.as_mut() {
                        popout.submenu = Submenu::Statuses;
                    }
                    cx.notify();
                },
                Submenu::Statuses,
            ),
            separator().into_any_element(),
            self.popout_row(
                "status-badges",
                icon("pm_badge", 16., "--interactive-text-default").into_any_element(),
                "Badges",
                None,
                false,
                cx,
                |this, _, cx| {
                    this.status_popout = None;
                    this.open_profile_editor(cx);
                },
                Submenu::None,
            ),
            separator().into_any_element(),
            self.popout_row(
                "status-orbs",
                icon("pm_orbs", 16., "--interactive-text-default").into_any_element(),
                "0 Orbs",
                Some(chevron()),
                false,
                cx,
                |this, _, cx| {
                    this.status_popout = None;
                    this.open_settings_page("nitro", cx);
                },
                Submenu::None,
            ),
        ]);
        let second = group(vec![self.popout_row(
            "status-switch-accounts",
            icon("pm_account", 16., "--interactive-text-default").into_any_element(),
            "Switch Accounts",
            Some(chevron()),
            popout.submenu == Submenu::Accounts,
            cx,
            |this, _, cx| {
                if let Some(popout) = this.status_popout.as_mut() {
                    popout.submenu = Submenu::Accounts;
                }
                cx.notify();
            },
            Submenu::Accounts,
        )]);

        let card = div()
            .id("status-popout")
            // The account popout opens with `animation: FADE` above its target (module 993382).
            .popout("status-popout", PopKind::Fade, PopSide::Top)
            .occlude()
            .w(px(300.))
            .rounded(px(8.))
            .bg(surface)
            .shadow(vec![
                BoxShadow {
                    color: gpui::hsla(0., 0., 1., 0.08),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(1.),
                    inset: false,
                },
                BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ])
            .pb(px(4.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .max_h_full()
            .child(header)
            .child(
                // The body is a scroller (`Ip` with `rf` in module 993382): a tall popout scrolls under its banner.
                div()
                    .id("status-popout-body")
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(16.))
                    .pt(px(4.))
                    .pb(px(18.))
                    .flex()
                    .flex_col()
                    // "Unable to load parts of profile" (module 559506, `nn` first in `body_nitlyF`, 12px apart).
                    .children(
                        self.profile_fetch_warning(user.id)
                            .map(|warning| div().mb(px(12.)).child(warning)),
                    )
                    .child(names)
                    .when(self.profile_upsell_shown(), |this| {
                        this.child(div().mt(px(6.)).child(super::profile::upsell_card_for(
                            self.premium().is_subscriber(),
                            cx,
                        )))
                    })
                    // Module 646986 (`ni` in 993382's body, after the bio): the account's own activity cards, the
                    // Spotify track among them, as the user popout draws them.
                    .children(
                        self.live_activity_cards(
                            user.id,
                            super::live_activity::Surface::Popout,
                            color("--text-strong").into(),
                            color("--text-subtle").into(),
                            cx,
                        )
                        .into_iter()
                        .map(|card| div().mt(px(12.)).child(card)),
                    )
                    .child(div().mt(px(12.)).child(first))
                    .child(div().mt(px(12.)).child(second)),
            );

        // The card's bottom edge is 72px above the window's: the user panel sits under it.
        let layer = div()
            .id("status-popout-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.press_outside(super::keyboard::Layer::StatusPopout, cx) {
                        cx.stop_propagation();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, _, _, cx| {
                    if this.press_outside(super::keyboard::Layer::StatusPopout, cx) {
                        cx.stop_propagation();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    this.press_outside(super::keyboard::Layer::StatusPopout, cx);
                }),
            )
            .child(
                div()
                    .absolute()
                    .left(px(8.))
                    .top(px(8.))
                    .bottom(px(72.))
                    .flex()
                    .flex_col()
                    .justify_end()
                    .child(card),
            )
            .children(self.status_submenu(popout.submenu, cx));
        Some(deferred(layer).with_priority(1).into_any_element())
    }

    /// The custom status bubble beside the avatar: the status as set, or the prompt that opens the modal.
    fn status_bubble(&self, text: Option<String>, cx: &mut Context<Self>) -> AnyElement {
        let surface = color("--background-surface-highest");
        let dot = |size: f32, left: f32, top: f32| {
            div()
                .absolute()
                .left(px(left))
                .top(px(top))
                .size(px(size))
                .rounded_full()
                .bg(surface)
                .border_1()
                .border_color(color("--border-muted"))
        };
        let label = text
            .clone()
            .unwrap_or_else(|| "Hit a high score in...".to_owned());
        div()
            .absolute()
            .left(px(105.))
            .top(px(98.))
            .max_w(px(181.))
            .child(
                div()
                    .id("status-bubble")
                    .relative()
                    .rounded(px(16.))
                    .border_1()
                    .border_color(color("--border-muted"))
                    .bg(surface)
                    .cursor_pointer()
                    .child(dot(20., 10., -8.))
                    .child(dot(10., -3., -15.))
                    .child(
                        div()
                            .px(px(12.))
                            .py(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .when(text.is_none(), |this| {
                                this.italic().child(icon("status_add", 16., "--text-muted"))
                            })
                            .child(label),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_custom_status(window, cx);
                        cx.stop_propagation();
                    })),
            )
            .into_any_element()
    }

    /// The status list at the popout's side, with the durations of the status under the pointer.
    fn status_submenu(&self, open: Submenu, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = matches!(open, Submenu::Statuses | Submenu::Durations(_));
        if open == Submenu::Accounts {
            return Some(self.accounts_submenu(cx));
        }
        if !shown {
            return None;
        }
        let menu_bg = menu_surface("--background-surface-higher");
        let row = |index: usize| {
            let (key, label, detail) = STATUSES[index];
            let lit = open == Submenu::Durations(index);
            let height = if detail.is_some() { 52. } else { 36. };
            div()
                .id(SharedString::from(format!("set-status-submenu-{key}")))
                .h(px(height))
                .pl(px(13.))
                .pr(px(8.))
                .rounded(px(2.))
                .flex()
                .items_center()
                .gap(px(13.))
                .cursor_pointer()
                .when(lit, |this| this.bg(color("--interactive-background-hover")))
                .hover(|style| style.bg(color("--interactive-background-hover")))
                .child(icon(status_menu_icon(key), 10., status_tint(key)))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(label),
                        )
                        .children(detail.map(|detail| {
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-muted"))
                                .child(detail)
                        })),
                )
                .when(key != "online", |this| {
                    this.child(icon("pm_chevron", 16., "--interactive-icon-default"))
                })
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    if *hovered && let Some(popout) = this.status_popout.as_mut() {
                        popout.submenu = if key == "online" {
                            Submenu::Statuses
                        } else {
                            Submenu::Durations(index)
                        };
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if key == "online" {
                        this.choose_status(key, None, cx);
                    } else if let Some(popout) = this.status_popout.as_mut() {
                        popout.submenu = Submenu::Durations(index);
                        cx.notify();
                    }
                    cx.stop_propagation();
                }))
        };
        let list = div()
            .id("status-menu")
            .occlude()
            .absolute()
            .left(px(296.))
            .bottom(px(79.))
            .w(px(300.))
            .p(px(9.))
            .rounded(px(8.))
            .bg(menu_bg)
            .border_1()
            .border_color(color("--border-subtle"))
            .shadow(vec![BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: point(px(0.), px(8.)),
                blur_radius: px(16.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(row(0))
            .child(div().my(px(8.)).h(px(1.)).bg(color("--border-subtle")))
            .child(row(1))
            .child(row(2))
            .child(row(3));
        let durations = match open {
            Submenu::Durations(index) => Some(self.duration_submenu(index, menu_bg, cx)),
            _ => None,
        };
        Some(
            div()
                .absolute()
                .size_full()
                .child(list)
                .children(durations)
                .into_any_element(),
        )
    }

    fn duration_submenu(
        &self,
        index: usize,
        menu_bg: gpui::Rgba,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = STATUSES[index].0;
        // Hung 9px above the hovered row: Idle's row starts 492 from the top of the window's 720.
        let top: f32 = 483. + [0., 36., 88.][index.saturating_sub(1)];
        // Kept inside the window, 8px from its bottom.
        let top = top.min(self.viewport.1 - 222. - 8.);
        div()
            .id("status-durations")
            .occlude()
            .absolute()
            .left(px(591.))
            .top(px(top))
            .w(px(188.))
            .p(px(9.))
            .rounded(px(8.))
            .bg(menu_bg)
            .border_1()
            .border_color(color("--border-subtle"))
            .shadow(vec![BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: point(px(0.), px(8.)),
                blur_radius: px(16.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .children(
                DURATIONS
                    .iter()
                    .enumerate()
                    .map(|(position, (label, duration))| {
                        let duration = *duration;
                        div()
                            .id(SharedString::from(format!(
                                "set-status-submenu-{key}--{position}"
                            )))
                            .h(px(34.))
                            .px(px(8.))
                            .rounded(px(2.))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .hover(|style| style.bg(color("--interactive-background-hover")))
                            .child(*label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.choose_status(key, duration, cx);
                                cx.stop_propagation();
                            }))
                    }),
            )
            .into_any_element()
    }

    fn accounts_submenu(&self, cx: &mut Context<Self>) -> AnyElement {
        self.accounts_submenu_view(cx)
    }

    pub(super) fn custom_status_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.custom_status.as_ref()?;
        let user = self.stores.current_user.clone().unwrap_or_default();
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let banner = self.banner_color(&user);
        let status = self.stores.status.clone();
        let text = open.input.read(cx).text().to_owned();
        let preview = div().mt(px(8.)).flex().justify_center().child(
            div()
                .relative()
                .w(px(300.))
                .h(px(219.))
                .rounded(px(8.))
                .bg(color("--background-surface-high"))
                .border_1()
                .border_color(color("--border-subtle"))
                .child(div().h(px(105.)).rounded_t(px(8.)).bg(banner))
                .child(
                    div()
                        .absolute()
                        .left(px(10.))
                        .top(px(55.))
                        .size(px(92.))
                        .rounded_full()
                        .bg(color("--background-surface-high"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .relative()
                                .size(px(80.))
                                .rounded_full()
                                .children(
                                    avatar.map(|picture| img(picture).size(px(80.)).rounded_full()),
                                )
                                .child(status_dot_large(&status, "--background-surface-high")),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(105.))
                        .top(px(98.))
                        .max_w(px(181.))
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(16.))
                        .bg(color("--background-surface-highest"))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .italic()
                        .text_color(color("--text-muted"))
                        .child(if text.is_empty() {
                            "Hit a high score in...".to_owned()
                        } else {
                            text
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(16.))
                        .top(px(151.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .h(px(24.))
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(user.display_name().to_owned()),
                        )
                        .child(
                            div()
                                .h(px(24.))
                                .text_size(px(14.))
                                .line_height(px(24.))
                                .text_color(color("--text-strong"))
                                .child(user.username.clone()),
                        ),
                ),
        );
        let field = div()
            .mt(px(36.))
            .pb(px(20.))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(20.))
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Status"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .h(px(47.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-focus"))
                    .bg(color("--input-background-default"))
                    .pl(px(14.))
                    .pr(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(icon("pm_emoji", 24., "--interactive-text-default"))
                    .child(div().flex_1().min_w(px(0.)).child(open.input.clone())),
            );
        let clear = open.clear;
        let trigger = div()
            .id("custom-status-clear")
            .relative()
            .h(px(40.))
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-strong"))
            .child(clear_label(clear))
            .child(icon("chevron_down", 16., "--interactive-icon-default"))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(open) = this.custom_status.as_mut() {
                    open.clear_open = !open.clear_open;
                }
                cx.notify();
                cx.stop_propagation();
            }))
            .when(open.clear_open, |this| {
                this.child(
                    deferred(
                        anchored().child(
                            div()
                                .id("custom-status-clear-list")
                                .occlude()
                                .mt(px(-4.))
                                .w(px(260.))
                                .p(px(8.))
                                .rounded(px(8.))
                                .bg(color("--background-surface-higher"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .children(ClearAfter::ALL.iter().map(|choice| {
                                    let choice = *choice;
                                    div()
                                        .id(SharedString::from(format!("clear-after-{choice:?}")))
                                        .h(px(32.))
                                        .px(px(8.))
                                        .rounded(px(4.))
                                        .flex()
                                        .items_center()
                                        .cursor_pointer()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--interactive-text-default"))
                                        .hover(|style| {
                                            style
                                                .bg(color("--interactive-background-hover"))
                                                .text_color(color("--text-strong"))
                                        })
                                        .child(clear_option(choice))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(open) = this.custom_status.as_mut() {
                                                open.clear = choice;
                                                open.clear_open = false;
                                            }
                                            cx.notify();
                                            cx.stop_propagation();
                                        }))
                                })),
                        ),
                    )
                    .with_priority(3),
                )
            })
            .into_any_element();
        let spec = modal::Spec {
            id: "custom-status",
            title: "Set your status".into(),
            subtitle: None,
            body: Some(div().child(preview).child(field).into_any_element()),
            buttons: vec![modal::button(
                "custom-status-save",
                "Save",
                Tone::Primary,
                |this, _, cx| {
                    this.save_custom_status(cx);
                },
            )],
            close: Box::new(|this, _, _| this.custom_status = None),
        };
        Some(modal::render_with_input(spec, trigger, cx))
    }
}

fn status_tint(status: &str) -> &'static str {
    match status {
        "dnd" => "--icon-status-dnd",
        "idle" => "--icon-status-idle",
        "invisible" | "offline" => "--icon-status-offline",
        _ => "--icon-status-online",
    }
}

fn status_menu_icon(status: &str) -> &'static str {
    // Not in the pinned client: ColorSighted draws every status as the plain disk (`plugin_hooks.rs`).
    if super::plugin_hooks::color_sighted() {
        return "status_menu_online";
    }
    match status {
        "dnd" => "status_menu_dnd",
        "idle" => "status_menu_idle",
        "invisible" => "status_menu_offline",
        _ => "status_menu_online",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_lapses_after_its_length_and_forever_never() {
        assert_eq!(expires_at(1_000, Some(15 * MINUTE)), 1_000 + 900_000);
        assert_eq!(expires_at(1_000, None), 0);
        // The ids of the official menu name these lengths in milliseconds.
        let lengths: Vec<Option<u64>> = DURATIONS.iter().map(|(_, length)| *length).collect();
        assert_eq!(
            lengths,
            [
                Some(900_000),
                Some(3_600_000),
                Some(28_800_000),
                Some(86_400_000),
                Some(259_200_000),
                None
            ]
        );
    }

    #[test]
    fn the_clear_choice_follows_the_status_already_set() {
        let now = 1_000_000;
        assert_eq!(ClearAfter::from_expiry(0, now, false), ClearAfter::Never);
        assert_eq!(
            ClearAfter::from_expiry(now + 10, now, false),
            ClearAfter::Today
        );
        assert_eq!(
            ClearAfter::from_expiry(now + 20 * MINUTE, now, true),
            ClearAfter::Minutes30
        );
        assert_eq!(
            ClearAfter::from_expiry(now + 50 * MINUTE, now, true),
            ClearAfter::Hour1
        );
        assert_eq!(
            ClearAfter::from_expiry(now + 3 * HOUR, now, true),
            ClearAfter::Hours4
        );
        assert_eq!(
            ClearAfter::from_expiry(now + 6 * HOUR, now, true),
            ClearAfter::Today
        );
    }

    #[test]
    fn the_clear_options_read_like_the_dropdown() {
        assert_eq!(clear_option(ClearAfter::Never), "Don't clear");
        assert!(clear_option(ClearAfter::Today).starts_with("24 hours ("));
        assert!(clear_option(ClearAfter::Minutes30).starts_with("30 minutes ("));
        assert!(clear_option(ClearAfter::Hour1).starts_with("1 hour ("));
        assert_eq!(clear_label(ClearAfter::Never), "Don't clear");
    }
}
