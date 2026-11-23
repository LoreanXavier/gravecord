//! The overlay's panels and menus, in the Onyx theme:
//!
//! - The Widgets panel (module 776162, stylesheet 255324 `panel_fzU3RU`): 360 px at the top right under the toolbar,
//!   "Widgets" / "Choose what shows up in your overlay." and a close button, then the "Discord" section with one row
//!   per listed widget (`dc_overlay::widgets::panel`): its 32 px icon and a switch with the label and description,
//!   on while the layout holds the widget not hidden (`$e`), switched with `JA` (`Layout::set_enabled`). The "Your
//!   Apps" section lists Conjure projects, which this client has none of.
//! - The widgets' Settings menus (modules 51444, 320490; menu styles of module 798819 `menu_M3qk4j`): opened at the
//!   pointer, writing the stores the Game Overlay page writes (`dc_overlay::menus`).
//! - The Report a Problem dialog (module 967055): Overlay Issue (the category select, required unless details are
//!   given), Steps & Details, System Details (the game and its fullscreen type). Submit sends the
//!   `overlay_bug_report` analytics event (`POST /science`), which an inert run refuses like any other write.

use super::overlay::onyx;
use super::{Editor, Root, editor_style};
use crate::net::Request;
use dc_overlay::decision::FullscreenType;
use dc_overlay::menus::{self, Action, Menu};
use dc_overlay::store::{AvatarSize, DisplayNameMode, DisplayUserMode};
use dc_overlay::widgets::{self, ChatConfig};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, div, prelude::*, px, rgba,
    svg,
};
use serde_json::{Value, json};

/// `fixed_M3qk4j`: a menu without `variant` is 220 px wide.
const MENU_WIDTH: f32 = 220.;
/// `--custom-menu-viewport-padding`.
const MENU_EDGE: f32 = 16.;
/// The slider's track inside the menu's 8 px row padding.
const SLIDER_WIDTH: f32 = MENU_WIDTH - 2. - 16. - 16.;

/// An open Settings menu.
pub(super) struct OverlayMenu {
    pub kind: Menu,
    /// Where it opened (the pointer, logical px of the overlay window).
    pub at: (f32, f32),
    /// The Max Users slider is held.
    pub sliding: bool,
}

/// The Report a Problem dialog's state.
pub(super) struct ReportDialog {
    /// The issue category (`ef`), by index.
    pub category: Option<usize>,
    pub details: Entity<Editor>,
    /// The game picked (`el`): its name.
    pub game: Option<String>,
    pub fullscreen: Option<FullscreenType>,
    /// Which select is open: 0 the category, 1 the game, 2 the fullscreen type.
    pub open: Option<u8>,
    /// Submit was pressed with nothing chosen and nothing written (`eo`).
    pub missing: bool,
    pub sent: bool,
}

/// `ef` of module 967055: the categories in their order, with their message keys.
pub(super) const CATEGORIES: [(&str, &str); 8] = [
    ("game_crashes_occurred", "1UWmCV"),
    ("poor_performance", "4owu+4"),
    ("overlay_unresponsive", "Q8Z0xo"),
    ("keybindings_not_working", "UnzaZs"),
    ("alt_tabbing_issues", "/sG9Sf"),
    ("game_not_interactable", "+18gq6"),
    ("game_forced_minimized", "dabM8C"),
    ("other", "UhbPA+"),
];

/// `Z` of module 967055 and `ey`: the fullscreen types, in the set's order, with their message keys and wire names.
pub(super) const FULLSCREEN: [(FullscreenType, &str, &str); 6] = [
    (
        FullscreenType::BorderlessFullscreen,
        "NwNvjf",
        "BORDERLESS_FULLSCREEN",
    ),
    (FullscreenType::Fullscreen, "AjBsPe", "FULLSCREEN"),
    (FullscreenType::Windowed, "0oYq2i", "WINDOWED"),
    (FullscreenType::Maximized, "MUX2+G", "MAXIMIZED"),
    (FullscreenType::Minimized, "Wypflu", "MINIMIZED"),
    (FullscreenType::Unknown, "QPOep8", "UNKNOWN"),
];

/// Submit's check: nothing chosen and nothing written shakes the label and says why.
pub(super) fn report_ready(category: Option<usize>, details: &str) -> bool {
    category.is_some() || !details.is_empty()
}

/// The `overlay_bug_report` event as module 967055 builds it, with what this client knows (no native module state,
/// breadcrumbs or experiment buckets).
pub(super) fn report_event(
    category: Option<usize>,
    details: &str,
    game: Option<&str>,
    fullscreen: Option<FullscreenType>,
) -> Value {
    let fullscreen = FULLSCREEN
        .iter()
        .find(|(kind, _, _)| Some(*kind) == fullscreen)
        .map_or("UNKNOWN", |(_, _, wire)| wire);
    json!({
        "events": [{
            "type": "overlay_bug_report",
            "properties": {
                "issue_category": category.and_then(|at| CATEGORIES.get(at)).map(|(name, _)| *name),
                "details": details,
                "overlay_render_method": "OutOfProcess",
                "detected_fullscreen_type": fullscreen,
                "application_name": game,
                "location": "overlay",
            }
        }]
    })
}

/// Where a menu of `size` opening at `at` sits in a `window`: it keeps the viewport padding, moving up or left
/// rather than flipping.
pub(super) fn menu_origin(at: (f32, f32), size: (f32, f32), window: (f32, f32)) -> (f32, f32) {
    let left = at.0.min(window.0 - size.0 - MENU_EDGE).max(MENU_EDGE);
    let top = at.1.min(window.1 - size.1 - MENU_EDGE).max(MENU_EDGE);
    (left, top)
}

fn text(size: f32, line: f32, weight: f32, token: &str) -> gpui::Div {
    div()
        .text_size(px(size))
        .line_height(px(line))
        .font_weight(FontWeight(weight))
        .text_color(onyx(token))
}

/// The Onyx switch of a form row (48 x 24, as the settings pages').
fn switch(on: bool) -> gpui::Div {
    div()
        .flex_none()
        .relative()
        .w(px(48.))
        .h(px(24.))
        .rounded(px(16.))
        .border_1()
        .border_color(onyx("--input-border-default"))
        .bg(if on {
            onyx("--background-brand")
        } else {
            onyx("--input-background-default")
        })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 26. } else { 2. }))
                .size(px(18.))
                .rounded_full()
                .bg(gpui::white()),
        )
}

/// A menu radio (`radioIcon_M3qk4j`, 20 px, `--control-brand-foreground-new` when chosen; drawn as a ring with a dot,
/// inferred: the icon's own shape is not in the dumps).
fn radio(on: bool) -> gpui::Div {
    let tint = if on {
        onyx("--control-brand-foreground-new")
    } else {
        onyx("--interactive-text-default")
    };
    div()
        .size(px(20.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(18.))
                .rounded_full()
                .border_2()
                .border_color(tint)
                .flex()
                .items_center()
                .justify_center()
                .when(on, |this| {
                    this.child(div().size(px(8.)).rounded_full().bg(tint))
                }),
        )
}

fn checkbox(on: bool) -> gpui::Div {
    div()
        .size(px(20.))
        .flex_none()
        .rounded(px(4.))
        .border_1()
        .border_color(if on {
            onyx("--control-brand-foreground-new")
        } else {
            onyx("--interactive-text-default")
        })
        .when(on, |this| this.bg(onyx("--control-brand-foreground-new")))
        .flex()
        .items_center()
        .justify_center()
        .when(on, |this| {
            this.child(
                svg()
                    .path("icons/checkbox_check.svg")
                    .size(px(14.))
                    .text_color(onyx("--white")),
            )
        })
}

impl Root {
    /// `Ay` of module 776162 (`panel_fzU3RU` / `panelContent_fzU3RU`).
    pub(super) fn overlay_widgets_panel(
        &self,
        height: f32,
        config: ChatConfig,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = self.overlay_layout();
        let rows = widgets::panel(config).into_iter().map(|entry| {
            let on = layout.is_enabled(entry.kind);
            let kind = entry.kind;
            div()
                .id(SharedString::from(format!("overlay-panel-{kind}")))
                .flex()
                .items_center()
                .gap(px(12.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.overlay_change_layout(|layout| {
                        layout.set_enabled(kind, !on, &mut super::overlay::new_widget_id);
                        true
                    });
                    tracing::debug!(
                        "overlay widget {kind} {}",
                        if on { "removed" } else { "added" }
                    );
                    cx.notify();
                }))
                .child(
                    div()
                        .size(px(32.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path(format!("icons/{}.svg", entry.icon))
                                .size(px(24.))
                                .text_color(onyx("--interactive-text-default")),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(
                                    text(16., 20., 500., "--text-strong")
                                        .child(dc_intl::tr(entry.label)),
                                )
                                .child(
                                    text(14., 18., 400., "--text-muted")
                                        .child(dc_intl::tr(entry.description)),
                                ),
                        )
                        .child(switch(on)),
                )
        });
        div()
            .id("overlay-widgets-panel")
            .absolute()
            .top(px(24. + 48.))
            .right(px(24.))
            .w(px(360.))
            .max_h(px((height - 24. * 2. - 48.).max(0.)))
            .overflow_y_scroll()
            .p(px(16.))
            .rounded(px(16.))
            .border_1()
            .border_color(onyx("--border-subtle"))
            .bg(onyx("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_start()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        text(20., 24., 600., "--text-strong")
                                            .child(dc_intl::tr("Widgets")),
                                    )
                                    .child(text(14., 18., 400., "--text-muted").child(
                                        dc_intl::tr("Choose what shows up in your overlay."),
                                    )),
                            )
                            .child(
                                div()
                                    .id("overlay-widgets-panel-close")
                                    .size(px(32.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(onyx("--interactive-background-hover")))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.overlay.widgets_panel = false;
                                        cx.notify();
                                    }))
                                    .child(
                                        svg()
                                            .path("icons/overlay_x_small_sm.svg")
                                            .size(px(20.))
                                            .text_color(onyx("--interactive-text-default")),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(text(12., 16., 600., "--text-muted").child(dc_intl::t("/7xJCF")))
                            .children(rows),
                    ),
            )
            .into_any_element()
    }

    /// The Settings button of a widget's extras.
    pub(super) fn overlay_open_menu(&mut self, kind: Menu) {
        let open = self
            .overlay
            .menu
            .as_ref()
            .is_some_and(|menu| menu.kind == kind);
        self.overlay.menu = (!open).then_some(OverlayMenu {
            kind,
            at: self.overlay.pointer,
            sliding: false,
        });
    }

    /// One choice of a menu: the store it belongs to is written, as the page does it.
    pub(super) fn overlay_menu_action(&mut self, action: Action) {
        // The video and Go Live menus' rows act on their own widget (modules 915908, 871648, 575079).
        let kind = match self.overlay.menu.as_ref().map(|menu| menu.kind) {
            Some(Menu::Video) => Some(dc_overlay::layout::VIDEO),
            Some(Menu::GoLive) => Some(dc_overlay::layout::GO_LIVE),
            _ => None,
        };
        match action {
            Action::ShowOwnCamera => {
                // GAP: hiding our own camera asks first in the official client (module 513963) unless that alert was
                // turned off; the app's own "Show My Own Camera" rows do not ask either.
                super::voice_view::toggle_call_setting(super::voice_view::SHOW_OWN_CAMERA);
                return;
            }
            Action::Horizontal | Action::Opacity(_) => {
                if let Some(widget) = kind.and_then(|kind| self.overlay_layout().widget(kind)) {
                    let window = self.overlay.size;
                    self.overlay_change_layout(|layout| {
                        let changed = menus::apply_widget(layout, &widget.id, action);
                        // `iS`'s layout effect: the video widget turned takes the size that suits it (`iE`).
                        if changed
                            && action == Action::Horizontal
                            && widget.kind == dc_overlay::layout::VIDEO
                        {
                            let (width, height) = dc_overlay::drag::requested_size(
                                widget.size.pixels(window.0, window.1),
                                widget.min_size,
                            );
                            let size = (
                                width.unwrap_or(widget.min_size.0),
                                height.unwrap_or(widget.min_size.1),
                            );
                            let inset = 2. * (dc_overlay::drag::PADDING + dc_overlay::drag::BORDER);
                            let content = ((size.0 - inset).max(0.), (size.1 - inset).max(0.));
                            if let Some((width, height)) = dc_overlay::video::video_reoriented(
                                !widget.horizontal(),
                                size,
                                content,
                                window,
                            ) {
                                layout.update_widget(
                                    &widget.id,
                                    None,
                                    Some(dc_overlay::layout::Size {
                                        width: dc_overlay::layout::Extent::Value(width),
                                        height: dc_overlay::layout::Extent::Value(height),
                                        fixed: true,
                                    }),
                                );
                            }
                        }
                        changed
                    });
                }
                // `Z_`: Horizontal Layout closes the menu.
                if action == Action::Horizontal {
                    self.overlay.menu = None;
                }
                return;
            }
            _ => {}
        }
        let settings = self.overlay_settings();
        if let Some(next) = menus::apply_settings(&settings, action) {
            self.set_overlay_settings(&next);
            return;
        }
        let stored = crate::storage::get(dc_overlay::store::SETTINGS_STORE).unwrap_or(Value::Null);
        if let Some(next) = menus::apply_notification_settings(&stored, action) {
            crate::storage::set(dc_overlay::store::SETTINGS_STORE, next);
            return;
        }
        self.overlay_change_layout(|layout| {
            menus::apply_layout(layout, action, &mut super::overlay::new_widget_id)
        });
    }

    /// The Max Users slider held at `x` (logical px of the window).
    pub(super) fn overlay_menu_slide(&mut self, x: f32) {
        let track = self
            .overlay
            .bounds
            .borrow()
            .get("overlay-menu-slider")
            .copied();
        if let Some(bounds) = track {
            let width = f32::from(bounds.size.width).max(1.);
            let fraction = ((x - f32::from(bounds.origin.x)) / width).clamp(0., 1.);
            let action = match self.overlay.menu.as_ref().map(|menu| menu.kind) {
                Some(Menu::Video | Menu::GoLive) => Action::Opacity(fraction * menus::OPACITY_MAX),
                _ => Action::MaxUsers(menus::max_users_at(fraction)),
            };
            self.overlay_menu_action(action);
        }
    }

    fn overlay_menu_row(
        &self,
        id: String,
        label: String,
        mark: gpui::Div,
        action: Action,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(SharedString::from(id))
            .min_h(px(32.))
            .p(px(8.))
            .flex()
            .items_center()
            .justify_between()
            .rounded(px(2.))
            .cursor_pointer()
            .hover(|style| {
                style
                    .bg(onyx("--interactive-background-hover"))
                    .rounded(px(4.))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.overlay_menu_action(action);
                cx.notify();
            }))
            .child(
                text(14., 18., 500., "--text-strong")
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(label),
            )
            .child(div().ml(px(8.)).child(mark))
            .into_any_element()
    }

    fn overlay_menu_group(label: Option<&'static str>, rows: Vec<AnyElement>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .when_some(label, |this, label| {
                this.child(
                    div()
                        .min_h(px(32.))
                        .p(px(8.))
                        .flex()
                        .items_center()
                        .child(text(14., 20., 500., "--text-muted").child(dc_intl::t(label))),
                )
            })
            .children(rows)
            .into_any_element()
    }

    /// The open Settings menu (`overlay-voice-widget-context-menu` or `overlay-notifications-widget-context-menu`).
    pub(super) fn overlay_menu_layer(
        &self,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.overlay.menu.as_ref()?;
        let settings = self.overlay_settings();
        let notifications =
            crate::storage::get(dc_overlay::store::SETTINGS_STORE).unwrap_or(Value::Null);
        let row = |this: &Self,
                   id: &str,
                   key: &str,
                   action: Action,
                   radio_mark: bool,
                   cx: &mut Context<Self>| {
            let on = menus::checked(action, &settings, &notifications);
            this.overlay_menu_row(
                id.to_owned(),
                dc_intl::t(key).to_owned(),
                if radio_mark { radio(on) } else { checkbox(on) },
                action,
                cx,
            )
        };
        let separator = || {
            div()
                .m(px(8.))
                .h(px(1.))
                .bg(onyx("--border-subtle"))
                .into_any_element()
        };
        let mut items: Vec<AnyElement> = Vec::new();
        match menu.kind {
            Menu::Voice => {
                items.push(Self::overlay_menu_group(
                    Some("dnvZSg"),
                    vec![
                        row(
                            self,
                            "avatar-size-mode-large",
                            "YcOxtr",
                            Action::AvatarSize(AvatarSize::Large),
                            true,
                            cx,
                        ),
                        row(
                            self,
                            "avatar-size-mode-small",
                            "BKIKqx",
                            Action::AvatarSize(AvatarSize::Small),
                            true,
                            cx,
                        ),
                    ],
                ));
                items.push(separator());
                items.push(Self::overlay_menu_group(
                    Some("J0dpcB"),
                    vec![
                        row(
                            self,
                            "display-name-mode-ALWAYS",
                            "nBmDrT",
                            Action::DisplayNames(DisplayNameMode::Always),
                            true,
                            cx,
                        ),
                        row(
                            self,
                            "display-name-mode-ONLY_WHILE_SPEAKING",
                            "2OvIZY",
                            Action::DisplayNames(DisplayNameMode::OnlyWhileSpeaking),
                            true,
                            cx,
                        ),
                        row(
                            self,
                            "display-name-mode-NEVER",
                            "ekjlPL",
                            Action::DisplayNames(DisplayNameMode::Never),
                            true,
                            cx,
                        ),
                    ],
                ));
                items.push(separator());
                items.push(Self::overlay_menu_group(
                    Some("swsWWC"),
                    vec![
                        row(
                            self,
                            "display-user-mode-ALWAYS",
                            "nBmDrT",
                            Action::DisplayUsers(DisplayUserMode::Always),
                            true,
                            cx,
                        ),
                        row(
                            self,
                            "display-user-mode-ONLY_WHILE_SPEAKING",
                            "2OvIZY",
                            Action::DisplayUsers(DisplayUserMode::OnlyWhileSpeaking),
                            true,
                            cx,
                        ),
                    ],
                ));
                items.push(separator());
                items.push(self.overlay_menu_slider(cx));
            }
            Menu::Notifications => {
                items.push(row(
                    self,
                    "overlay-text-chat-notifications",
                    "Fy5kPp",
                    Action::ToggleTextChatNotifications,
                    false,
                    cx,
                ));
            }
            // Modules 662554 and 543824: Show My Own Camera (video only), Horizontal Layout, Opacity.
            Menu::Video | Menu::GoLive => {
                let kind = if menu.kind == Menu::Video {
                    dc_overlay::layout::VIDEO
                } else {
                    dc_overlay::layout::GO_LIVE
                };
                let widget = self.overlay_layout().widget(kind);
                if menu.kind == Menu::Video {
                    let on = super::voice_view::call_setting(super::voice_view::SHOW_OWN_CAMERA);
                    items.push(self.overlay_menu_row(
                        "self-video-hide".to_owned(),
                        dc_intl::t("MH8ESU").to_owned(),
                        checkbox(on),
                        Action::ShowOwnCamera,
                        cx,
                    ));
                }
                let horizontal = widget.as_ref().is_some_and(|widget| widget.horizontal());
                items.push(self.overlay_menu_row(
                    "widget-layout-type".to_owned(),
                    dc_intl::t("XQQQFE").to_owned(),
                    checkbox(horizontal),
                    Action::Horizontal,
                    cx,
                ));
                items.push(self.overlay_opacity_slider(
                    widget.as_ref().map_or(1., |widget| widget.opacity),
                    cx,
                ));
            }
        }
        let measured = self
            .overlay
            .bounds
            .borrow()
            .get("overlay-menu")
            .map_or(0., |bounds| f32::from(bounds.size.height));
        let (left, top) = menu_origin(menu.at, (MENU_WIDTH, measured), (width, height));
        Some(
            div()
                .id("overlay-menu")
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(MENU_WIDTH))
                .py(px(8.))
                .px(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(onyx("--border-subtle"))
                .bg(onyx("--background-surface-higher"))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: gpui::point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .occlude()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                // The menu occludes the window under it: a slider held inside the menu follows the pointer here.
                .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                    if this.overlay.menu.as_ref().is_some_and(|menu| menu.sliding)
                        && event.dragging()
                    {
                        this.overlay_menu_slide(f32::from(event.position.x));
                        cx.notify();
                    }
                }))
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, _| {
                        if let Some(menu) = this.overlay.menu.as_mut() {
                            menu.sliding = false;
                        }
                    }),
                )
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    if this.overlay.menu.as_ref().is_some_and(|menu| !menu.sliding) {
                        this.overlay.menu = None;
                        cx.notify();
                    }
                }))
                .child(super::overlay_widgets::probe(
                    &self.overlay.bounds,
                    "overlay-menu".into(),
                ))
                .children(items)
                .into_any_element(),
        )
    }

    /// Max Users Displayed (`voice-widget-max-users-displayed`): top label only, 58px height, `5px` inset
    /// track (`4->6px` bar, `16->20px` grabber), and live tooltip bubble while sliding.
    fn overlay_menu_slider(&self, cx: &mut Context<Self>) -> AnyElement {
        let stored = self.overlay_layout().voice_max_shown();
        let fraction = menus::max_users_fraction(stored);
        let value = match menus::max_users_label(stored as f32) {
            Some(count) => count.to_string(),
            None => dc_intl::t!("nrUzFL").to_owned(),
        };
        self.overlay_slider_row(
            "voice-widget-max-users-displayed",
            dc_intl::t!("X/Uyzc").to_owned(),
            fraction,
            value,
            cx,
        )
    }

    /// Opacity (module 871648, `opacity`): the same slider row, 0 to 100.
    fn overlay_opacity_slider(&self, opacity: f32, cx: &mut Context<Self>) -> AnyElement {
        self.overlay_slider_row(
            "opacity",
            dc_intl::t("OVovCb").to_owned(),
            menus::opacity_fraction(opacity),
            menus::opacity_label(opacity).to_string(),
            cx,
        )
    }

    fn overlay_slider_row(
        &self,
        id: &'static str,
        label: String,
        fraction: f32,
        value: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.overlay.menu.as_ref().is_some_and(|menu| menu.sliding);
        let (bar, grabber) = super::menu_slider::track_layers(
            "overlay-max-users-slider",
            fraction,
            active,
            super::menu_slider::ThemePalette::Onyx,
        );
        let slider = div()
            .id("overlay-menu-slider")
            .relative()
            .mx(px(8.))
            .w(px(SLIDER_WIDTH))
            .h(px(super::menu_slider::SLIDER_HEIGHT))
            .cursor_ew_resize()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if let Some(menu) = this.overlay.menu.as_mut() {
                        menu.sliding = true;
                    }
                    this.overlay_menu_slide(f32::from(event.position.x));
                    cx.notify();
                }),
            )
            .child(bar)
            .child(
                div()
                    .absolute()
                    .left(px(super::menu_slider::TRACK_INSET))
                    .right(px(super::menu_slider::TRACK_INSET))
                    .top_0()
                    .bottom_0()
                    .child(super::overlay_widgets::probe(
                        &self.overlay.bounds,
                        "overlay-menu-slider".into(),
                    ))
                    .child(grabber)
                    .when(active, |this| {
                        this.child(
                            div()
                                .absolute()
                                .left(gpui::relative(fraction))
                                .bottom(px(super::menu_slider::SLIDER_HEIGHT + 6.))
                                .ml(px(-20.))
                                .min_w(px(40.))
                                .px(px(12.))
                                .py(px(6.))
                                .rounded(px(8.))
                                .bg(onyx("--background-surface-highest"))
                                .border_1()
                                .border_color(onyx("--border-subtle"))
                                .shadow(vec![gpui::BoxShadow {
                                    color: gpui::hsla(0., 0., 0., 0.24),
                                    offset: gpui::point(px(0.), px(4.)),
                                    blur_radius: px(8.),
                                    spread_radius: px(0.),
                                    inset: false,
                                }])
                                .flex()
                                .justify_center()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(gpui::FontWeight(500.))
                                .text_color(onyx("--text-strong"))
                                .whitespace_nowrap()
                                .child(value),
                        )
                    }),
            )
            .into_any_element();
        super::menu_slider::row_frame(id, label, super::menu_slider::ThemePalette::Onyx, slider)
            .into_any_element()
    }

    /// The toolbar's Report button (`OKmenM`).
    pub(super) fn overlay_open_report(&mut self, cx: &mut Context<Self>) {
        let details = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder(dc_intl::t("68mJXW").to_owned());
            editor
        });
        let game = self.overlay_current_game().and_then(|game| game.name);
        let fullscreen = self.overlay.focused.map(|focused| focused.fullscreen);
        self.overlay.menu = None;
        self.overlay.report = Some(ReportDialog {
            category: None,
            details,
            game,
            fullscreen,
            open: None,
            missing: false,
            sent: false,
        });
        tracing::debug!("overlay: Report a Problem opened");
    }

    /// Submit (`geKm7t`): the event goes through the request queue, where an inert run refuses it.
    fn overlay_submit_report(&mut self, cx: &mut Context<Self>) {
        let Some(report) = self.overlay.report.as_mut() else {
            return;
        };
        let details = report.details.read(cx).text().to_owned();
        if !report_ready(report.category, &details) {
            report.missing = true;
            return;
        }
        let body = report_event(
            report.category,
            &details,
            report.game.as_deref(),
            report.fullscreen,
        );
        report.sent = true;
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: "/science".to_owned(),
            body: Some(body),
        });
        tracing::info!("overlay: a problem report was submitted");
        self.overlay.report = None;
    }

    fn overlay_select(
        &self,
        index: u8,
        placeholder: String,
        chosen: Option<String>,
        options: Vec<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.overlay.report.as_ref().and_then(|report| report.open) == Some(index);
        div()
            .relative()
            .w_full()
            .child(
                div()
                    .id(SharedString::from(format!("overlay-report-select-{index}")))
                    .h(px(40.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(onyx("--input-border-default"))
                    .bg(onyx("--input-background-default"))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(report) = this.overlay.report.as_mut() {
                            report.open = if report.open == Some(index) {
                                None
                            } else {
                                Some(index)
                            };
                        }
                        cx.notify();
                    }))
                    .child(match chosen {
                        Some(chosen) => text(16., 20., 400., "--text-default").child(chosen),
                        None => text(16., 20., 400., "--input-placeholder-text-default")
                            .child(placeholder),
                    })
                    .child(
                        svg()
                            .path("icons/overlay_chevron_down.svg")
                            .size(px(16.))
                            .text_color(onyx("--interactive-text-default")),
                    ),
            )
            .when(open, |this| {
                this.child(
                    gpui::deferred(
                        div()
                            .id(SharedString::from(format!("overlay-report-list-{index}")))
                            .absolute()
                            .top(px(44.))
                            .left_0()
                            .w_full()
                            .max_h(px(208.))
                            .overflow_y_scroll()
                            .p(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(onyx("--border-subtle"))
                            .bg(onyx("--background-surface-higher"))
                            .shadow(vec![super::shadow_high()])
                            .occlude()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                if let Some(report) = this.overlay.report.as_mut() {
                                    report.open = None;
                                    cx.notify();
                                }
                            }))
                            .children(options.into_iter().enumerate().map(|(at, option)| {
                                div()
                                    .id(SharedString::from(format!(
                                        "overlay-report-option-{index}-{at}"
                                    )))
                                    .flex_none()
                                    .min_h(px(32.))
                                    .px(px(8.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(4.))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(onyx("--interactive-background-hover")))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        let games: Vec<String> = this
                                            .detect
                                            .store
                                            .running_games()
                                            .iter()
                                            .filter_map(|game| game.name.clone())
                                            .collect();
                                        if let Some(report) = this.overlay.report.as_mut() {
                                            match index {
                                                0 => {
                                                    report.category = Some(at);
                                                    report.missing = false;
                                                }
                                                1 => report.game = games.get(at).cloned(),
                                                _ => {
                                                    report.fullscreen =
                                                        FULLSCREEN.get(at).map(|(kind, _, _)| *kind)
                                                }
                                            }
                                            report.open = None;
                                        }
                                        cx.notify();
                                    }))
                                    .child(text(14., 18., 500., "--text-default").child(option))
                            })),
                    )
                    .with_priority(3),
                )
            })
            .into_any_element()
    }

    /// Module 967055's modal over the overlay.
    pub(super) fn overlay_report_layer(
        &self,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let report = self.overlay.report.as_ref()?;
        let label = |key: &str, critical: bool| {
            text(
                16.,
                20.,
                500.,
                if critical {
                    "--text-feedback-critical"
                } else {
                    "--text-strong"
                },
            )
            .child(dc_intl::t(key))
        };
        let category = report
            .category
            .and_then(|at| CATEGORIES.get(at))
            .map(|(_, key)| dc_intl::t(key).to_owned());
        let games: Vec<String> = self
            .detect
            .store
            .running_games()
            .iter()
            .filter_map(|game| game.name.clone())
            .collect();
        let fullscreen = FULLSCREEN
            .iter()
            .find(|(kind, _, _)| Some(*kind) == report.fullscreen)
            .map(|(_, key, _)| dc_intl::t(key).to_owned());
        let missing = report.missing;
        let button = |id: &'static str, key: &str, primary: bool| {
            div()
                .id(id)
                .h(px(38.))
                .min_w(px(96.))
                .px(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .bg(onyx(if primary {
                    "--control-primary-background-default"
                } else {
                    "--control-secondary-background-default"
                }))
                .text_color(onyx(if primary {
                    "--control-primary-text-default"
                } else {
                    "--control-secondary-text-default"
                }))
                .text_size(px(14.))
                .font_weight(FontWeight(500.))
                .child(dc_intl::t(key))
        };
        let modal =
            div()
                .id("overlay-report")
                .w(px(440.))
                .max_h(px((height - 32.).max(0.)))
                .overflow_y_scroll()
                .rounded(px(12.))
                .border_1()
                .border_color(onyx("--border-subtle"))
                .bg(onyx("--background-surface-high"))
                .occlude()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .p(px(24.))
                        .pb(px(16.))
                        .child(text(20., 24., 600., "--text-strong").child(dc_intl::t("OKmenM"))),
                )
                .child(
                    div()
                        .px(px(24.))
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(label("Trx7eJ", missing))
                                .child(
                                    self.overlay_select(
                                        0,
                                        dc_intl::t("mKTzjI").to_owned(),
                                        category,
                                        CATEGORIES
                                            .iter()
                                            .map(|(_, key)| dc_intl::t(key).to_owned())
                                            .collect(),
                                        cx,
                                    ),
                                )
                                .when(missing, |this| {
                                    this.child(
                                        text(12., 16., 600., "--text-feedback-critical")
                                            .child(dc_intl::t("5cqa9J")),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(label("iZufAY", false))
                                .child(
                                    div()
                                        .min_h(px(64.))
                                        .p(px(10.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(onyx("--input-border-default"))
                                        .bg(onyx("--input-background-default"))
                                        .text_color(onyx("--text-default"))
                                        .child(report.details.clone()),
                                )
                                .child(
                                    text(12., 16., 400., "--text-muted")
                                        .child(dc_intl::t("TzkM2a")),
                                ),
                        )
                        .child(div().h(px(1.)).bg(onyx("--border-subtle")))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(label("8+SU+a", false))
                                .child(self.overlay_select(
                                    1,
                                    dc_intl::t("b1IW2e").to_owned(),
                                    report.game.clone(),
                                    games,
                                    cx,
                                ))
                                .child(
                                    self.overlay_select(
                                        2,
                                        dc_intl::t("IjlqrF").to_owned(),
                                        fullscreen,
                                        FULLSCREEN
                                            .iter()
                                            .map(|(_, key, _)| dc_intl::t(key).to_owned())
                                            .collect(),
                                        cx,
                                    ),
                                ),
                        ),
                )
                .child(
                    div()
                        .p(px(16.))
                        .mt(px(16.))
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .child(button("overlay-report-cancel", "ETE/oC", false).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.overlay.report = None;
                                cx.notify();
                            }),
                        ))
                        .child(button("overlay-report-submit", "geKm7t", true).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.overlay_submit_report(cx);
                                cx.notify();
                            }),
                        )),
                );
        Some(
            div()
                .id("overlay-report-backdrop")
                .absolute()
                .top_0()
                .left_0()
                .w(px(width))
                .h(px(height))
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x000000b3))
                .occlude()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.overlay.report = None;
                        cx.notify();
                    }),
                )
                .child(modal)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_needs_a_category_or_details() {
        assert!(!report_ready(None, ""));
        assert!(report_ready(Some(0), ""));
        assert!(report_ready(None, "it froze"));
        let event = report_event(
            Some(4),
            "tabbed",
            Some("Demo Game"),
            Some(FullscreenType::BorderlessFullscreen),
        );
        let properties = &event["events"][0]["properties"];
        assert_eq!(event["events"][0]["type"], "overlay_bug_report");
        assert_eq!(properties["issue_category"], "alt_tabbing_issues");
        assert_eq!(
            properties["detected_fullscreen_type"],
            "BORDERLESS_FULLSCREEN"
        );
        assert_eq!(properties["application_name"], "Demo Game");
        let empty = report_event(None, "", None, None);
        assert_eq!(
            empty["events"][0]["properties"]["issue_category"],
            Value::Null
        );
        assert_eq!(
            empty["events"][0]["properties"]["detected_fullscreen_type"],
            "UNKNOWN"
        );
        assert_eq!(CATEGORIES.len(), 8);
    }

    #[test]
    fn a_menu_stays_inside_the_window() {
        assert_eq!(
            menu_origin((100., 100.), (220., 300.), (1280., 720.)),
            (100., 100.)
        );
        // Near the bottom right it moves up and left, keeping 16 px.
        assert_eq!(
            menu_origin((1200., 600.), (220., 300.), (1280., 720.)),
            (1044., 404.)
        );
        assert_eq!(
            menu_origin((-5., -5.), (220., 300.), (1280., 720.)),
            (16., 16.)
        );
    }

    #[test]
    fn a_report_submitted_in_an_inert_run_is_refused() {
        let request = Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: "/science".to_owned(),
            body: Some(report_event(Some(0), "", None, None)),
        };
        assert_eq!(request.inert(), crate::net::Inert::Deny);
    }
}
