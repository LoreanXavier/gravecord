//! The Game Overlay page (module 158935: `OVERLAY_*` settings, panel order Enable, General, Widgets, Voice Widget,
//! Notifications). Every control reads and writes the official store it belongs to (`crate::ui::overlay`):
//!
//! - Enable Overlay / Enable Legacy Overlay: `OverlayStore6` `{oopEnabled, legacyEnabled}` (`OVERLAY_SET_ENABLED`).
//! - Enable overlay clickable regions: `OverlayStoreV2[user].disableClickableRegions`, inverted.
//! - The widget switches and Reset Default Layout: `LayoutStore`'s `overlay_v3_default` layout (module 667419).
//! - Avatar Size, Display Names, Display Users: `OverlayStoreV2[user]`.
//! - Max Users Displayed: the voice widget's `meta.voiceStatesMaxShown` (-1 is Off).
//! - The notification switches: `OverlaySettingsStore.notificationSettings`, the set of disabled ones.
//! - Toggle Overlay Lock: the managed `TOGGLE_OVERLAY_INPUT_LOCK` keybind of `KeybindsStore`.
//! - The "Say hello to the new overlay." hero: dismissible content `OVERLAY_OOP_SETTINGS_NUX` (505).
//!
//! Styles: `groupContainer_DKMuWq` (the cards), `keybind*_DKMuWq` (the lock keybind), `widgetSettingsContainer_DKMuWq`
//! (the voice widget preview), `overlaySettingsNux_pz9Ex4` (the hero); values from
//! `design/dom/settings/game-overlay-darker-cozy.json`.

use super::model::{ButtonKind, Choice, Node};
use super::render::plain_switch;
use super::voice::shortcut_text;
use super::{Page, Root, button, color};
use crate::net::Request;
use crate::ui::overlay::{Row, RowLook, new_widget_id, voice_list, widget_header};
use dc_overlay::decision::OverlayMethod;
use dc_overlay::layout::{self, Layout};
use dc_overlay::store::{self, AvatarSize, DisplayNameMode, DisplayUserMode};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, div, img, prelude::*, px,
    rgba, svg,
};

/// `DismissibleContent.OVERLAY_OOP_SETTINGS_NUX` (module 554146).
const NUX: u16 = 505;
const NUX_FIRST: (&str, &str) = (
    "overlay_nux_first",
    "https://cdn.discordapp.com/assets/content/10b8ab47f3371360233219f4b20fa86155553ddb810ceb8688654738bf7e15d0.png",
);
const NUX_SETTINGS: (&str, &str) = (
    "overlay_nux_settings",
    "https://cdn.discordapp.com/assets/content/2aa57f16c71171fc8e0edb8cca60735f1192195344d17fa667de6d3ca8163ba0.png",
);
/// The preview's made-up people use the first default avatar (`456…` ids: `(id >> 22) % 6` is 0).
const DEFAULT_AVATAR: (&str, &str) = (
    "avatar:default:0",
    "https://cdn.discordapp.com/embed/avatars/0.png",
);

pub(in crate::ui) const OOP: &str = "overlay_oop_setting";
pub(in crate::ui) const LEGACY: &str = "overlay_legacy_setting";
const CLICKABLE: &str = "overlay_clickable_regions_setting";
const REPORT: &str = "overlay_bug_reporter_setting";
const KEYBIND: &str = "overlay_keybind_setting";
const PREVIEW: &str = "overlay_voice_widget_preview";
const AVATAR_SIZE: &str = "overlay_voice_widget_avatar_size";
const DISPLAY_NAMES: &str = "overlay_voice_widget_display_names";
const DISPLAY_USERS: &str = "overlay_voice_widget_display_users";
pub(in crate::ui) const MAX_USERS: &str = "overlay_voice_widget_max_users";

/// The pictures this page shows, asked for when settings open.
pub(super) fn requests() -> [(&'static str, &'static str); 3] {
    [NUX_FIRST, NUX_SETTINGS, DEFAULT_AVATAR]
}

/// The widget kind of a switch of the Widgets list (`fl`'s map, module 158935).
fn widget_kind(key: &str) -> Option<&'static str> {
    Some(match key {
        "overlay_widgets_voice" => layout::VOICE,
        "overlay_widgets_quick_actions" => layout::QUICK_ACTIONS,
        "overlay_widgets_notifications" => layout::NOTIFICATIONS,
        "overlay_widgets_chat" => layout::TEXT_CHAT,
        "overlay_widgets_activity" => layout::ACTIVITY,
        "overlay_widgets_video" => layout::VIDEO,
        "overlay_widgets_streams" => layout::GO_LIVE,
        _ => return None,
    })
}

/// The `disabledSetting` of a notification switch (`pF`, module 158935).
fn notification_setting(key: &str) -> Option<&'static str> {
    Some(match key {
        "overlay_notifications_text_chat" => store::TEXT_CHAT,
        "overlay_notifications_welcome" => store::WELCOME_GENERAL,
        "overlay_notifications_go_live" => store::GO_LIVE_NUDGE,
        "overlay_notifications_game_activity" => store::GAME_ACTIVITY,
        "overlay_notifications_now_playing" => store::NOW_PLAYING,
        _ => return None,
    })
}

/// The value of the Max Users slider: -1 (no limit) shows as Off, at the start of the track.
pub(super) fn max_users_value(layout: &Layout) -> f32 {
    let stored = layout.voice_max_shown();
    if stored < 1 { 0. } else { stored as f32 }
}

impl Root {
    /// The lock keybind as the overlay writes it (`dI(shortcut, true)`), `???` without one (module 427830 `th`).
    pub(in crate::ui) fn overlay_lock_text(&self) -> String {
        super::keybinds::overlay_lock()
            .filter(|keybind| !keybind.shortcut.is_empty())
            .map(|keybind| shortcut_text(&keybind.shortcut))
            .unwrap_or_else(|| "???".to_owned())
    }

    /// A switch of this page: the store's value. None for any other row.
    pub(super) fn overlay_switch_value(&self, key: &str) -> Option<bool> {
        if key == OOP {
            return Some(self.overlay_global().oop);
        }
        if key == LEGACY {
            return Some(self.overlay_global().legacy);
        }
        if key == CLICKABLE {
            return Some(!self.overlay_settings().disable_clickable_regions);
        }
        if let Some(kind) = widget_kind(key) {
            return Some(self.overlay_layout().is_enabled(kind));
        }
        notification_setting(key).map(|setting| !self.overlay_notification_disabled(setting))
    }

    /// A select of this page: the chosen option's index in the order the list shows them.
    pub(super) fn overlay_index(&self, key: &str) -> Option<usize> {
        let settings = self.overlay_settings();
        match key {
            AVATAR_SIZE => AvatarSize::OPTIONS
                .iter()
                .position(|mode| *mode == settings.avatar_size_mode),
            DISPLAY_NAMES => DisplayNameMode::OPTIONS
                .iter()
                .position(|mode| *mode == settings.display_name_mode),
            DISPLAY_USERS => DisplayUserMode::OPTIONS
                .iter()
                .position(|mode| *mode == settings.display_user_mode),
            _ => None,
        }
    }

    /// One of the two global switches, from its card.
    fn overlay_set_global(&mut self, key: &str, on: bool) {
        let mut global = self.overlay_global();
        // `a` of the Enable Overlay and Enable Legacy Overlay cards (158935): switching one off asks why (387101).
        let was = if key == OOP {
            global.oop
        } else {
            global.legacy
        };
        if was && !on {
            self.overlay_switched_off();
        }
        if key == OOP {
            global.oop = on;
        } else {
            global.legacy = on;
        }
        // `OVERLAY_SET_ENABLED`; the window follows at the next focus change or frame.
        crate::storage::set(store::ENABLED_STORE, store::enabled_json(global));
        self.overlay.sync_dirty = true;
    }

    /// A generated row of this page changed (its new value is in `setting_choices`): the store takes it. True when
    /// the key is one of this page's.
    pub(super) fn overlay_setting_changed(&mut self, key: &'static str) -> bool {
        let choice = self.setting_choices.get(key).copied();
        let on = choice == Some(Choice::On(true));
        let index = match choice {
            Some(Choice::Index(index)) => index,
            _ => 0,
        };
        if key == OOP || key == LEGACY {
            self.overlay_set_global(key, on);
        } else if key == CLICKABLE {
            // `OVERLAY_SET_DISABLE_CLICKABLE_REGIONS`.
            let mut settings = self.overlay_settings();
            settings.disable_clickable_regions = !on;
            self.set_overlay_settings(&settings);
        } else if let Some(kind) = widget_kind(key) {
            let mut layout = self.overlay_layout();
            layout.set_enabled(kind, on, &mut new_widget_id);
            self.save_overlay_layout(&layout);
        } else if let Some(setting) = notification_setting(key) {
            // `OVERLAY_SET_NOTIFICATION_DISABLED_SETTING`.
            self.set_overlay_notification_disabled(setting, !on);
        } else if [AVATAR_SIZE, DISPLAY_NAMES, DISPLAY_USERS].contains(&key) {
            let mut settings = self.overlay_settings();
            match key {
                AVATAR_SIZE => {
                    settings.avatar_size_mode = AvatarSize::OPTIONS[index.min(1)];
                }
                DISPLAY_NAMES => {
                    settings.display_name_mode = DisplayNameMode::OPTIONS[index.min(2)];
                }
                _ => settings.display_user_mode = DisplayUserMode::OPTIONS[index.min(1)],
            }
            self.set_overlay_settings(&settings);
        } else {
            return false;
        }
        // The page reads the store from now on.
        self.setting_choices.remove(key);
        true
    }

    /// Max Users Displayed was let go (`p$.setValue`).
    pub(super) fn overlay_slider_committed(&mut self, key: &str, value: f32) -> bool {
        if key != MAX_USERS {
            return false;
        }
        let mut layout = self.overlay_layout();
        layout.set_voice_max_shown(value, &mut new_widget_id);
        self.save_overlay_layout(&layout);
        self.setting_choices.remove(MAX_USERS);
        true
    }

    /// "Reset Default Layout" (`OVERLAY_WIDGETS_RESET`, module 667419 `eL`: `resetDefaultLayout(GJ, 4)`).
    pub(super) fn overlay_widgets_reset(&mut self) {
        let mut layout = self.overlay_layout();
        layout.reset(&mut new_widget_id);
        self.save_overlay_layout(&layout);
    }

    /// "Got it!" (`DISMISS` of the hero's dismissible content), a settings-proto write held back in a harness run.
    fn overlay_dismiss_nux(&mut self) {
        if self.stores.content_dismissed(NUX) {
            return;
        }
        let patch = self.stores.dismiss_content(NUX);
        if patch.is_empty() || crate::storage::settings_writes_blocked() {
            return;
        }
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: "/users/@me/settings-proto/1".into(),
            body: Some(serde_json::json!({"settings": patch})),
        });
    }

    /// The rows of this page drawn here, each with its 8px `baseSettingWrapper` padding. None for other rows.
    pub(super) fn overlay_row(
        &self,
        node: &'static Node,
        width: f32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let key = node.key();
        let on_page = self
            .settings
            .as_ref()
            .is_some_and(|open| open.page == Page::GameOverlay);
        if !on_page {
            return None;
        }
        Some(match key {
            "decoration" => self.overlay_hero(width, cx),
            // `OVERLAY_CURRENT_GAME` comes first in the Enable category, only while a game runs.
            OOP => {
                let card = self.overlay_switch_card(
                    OOP,
                    dc_intl::tr("Enable Overlay"),
                    dc_intl::tr(
                        "Lighter, faster, and more customizable. Requires Borderless display setting.",
                    ),
                    cx,
                );
                match self.overlay_current_game_card(cx) {
                    Some(game) => div()
                        .flex()
                        .flex_col()
                        .child(div().pt(px(8.)).pb(px(16.)).child(game))
                        .child(card)
                        .into_any_element(),
                    None => card,
                }
            }
            LEGACY => self.overlay_switch_card(
                LEGACY,
                dc_intl::tr("Enable Legacy Overlay"),
                dc_intl::tr("Legacy Overlay available for all supported games."),
                cx,
            ),
            REPORT => overlay_report_card(),
            KEYBIND => self.overlay_keybind_row(width, cx),
            PREVIEW => self.overlay_preview(),
            _ => return None,
        })
    }

    /// `overlaySettingsNux_pz9Ex4`: the two pictures, the title and its line, "Got it!" at the top right; then the
    /// page's 40px-margined divider. Gone once dismissed.
    fn overlay_hero(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        if self.stores.content_dismissed(NUX) {
            return div().into_any_element();
        }
        let picture = |(key, _): (&str, &str), height: f32, box_height: f32| {
            div().w(px(660.)).h(px(box_height)).flex_none().children(
                self.images
                    .get(key)
                    .cloned()
                    .map(|image| img(image).w(px(660.)).h(px(height))),
            )
        };
        div()
            .w(px(width))
            .flex()
            .flex_col()
            .child(
                div()
                    .relative()
                    .h(px(737.))
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(32.))
                    .child(picture(NUX_FIRST, 371., 374.))
                    .child(
                        div()
                            .w(px(360.))
                            .h(px(84.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(12.))
                            .text_center()
                            .child(
                                div()
                                    .text_size(px(24.))
                                    .line_height(px(32.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::tr("Say hello to the new overlay.")),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::tr(
                                        "We\u{2019}ve made the overlay lighter, faster, and more powerful \u{2014} fully customizable to fit your needs.",
                                    )),
                            ),
                    )
                    .child(picture(NUX_SETTINGS, 188., 191.))
                    .child(
                        div()
                            .id("overlay-nux-close")
                            .absolute()
                            .top(px(24.))
                            .right(px(12.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.overlay_dismiss_nux();
                                cx.notify();
                            }))
                            .child(button(
                                "overlay-nux-got-it",
                                dc_intl::tr("Got it!"),
                                ButtonKind::Primary,
                            )),
                    ),
            )
            .child(
                div()
                    .my(px(40.))
                    .h(px(1.))
                    .bg(color("--border-subtle")),
            )
            .into_any_element()
    }

    /// `groupContainer_DKMuWq standaloneContainer_DKMuWq`: title, line, the switch and the 20px spacer of the
    /// collapsed game list.
    fn overlay_switch_card(
        &self,
        key: &'static str,
        title: &'static str,
        line: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let on = self.overlay_switch_value(key).unwrap_or(true);
        let switch = plain_switch(
            SharedString::from(key),
            on,
            move |this, on| this.overlay_set_global(key, on),
            cx,
        );
        card(
            true,
            title,
            line,
            div()
                .w(px(100.))
                .flex()
                .justify_end()
                .items_center()
                .gap(px(8.))
                .child(switch)
                .child(div().size(px(20.)).flex_none())
                .into_any_element(),
        )
    }

    /// `keybindMainContainer_DKMuWq`: the label (and the only-shift warning), the recorder at the right.
    fn overlay_keybind_row(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let keybind = super::keybinds::overlay_lock();
        let shortcut = keybind
            .as_ref()
            .map(|keybind| keybind.shortcut.clone())
            .unwrap_or_default();
        let invalid = !super::keybinds::overlay_lock_valid(&shortcut);
        let recording = keybind
            .as_ref()
            .is_some_and(|keybind| self.kb_is_recording(&keybind.id));
        let text = if shortcut.is_empty() {
            dc_intl::tr("No Keybind Set").to_owned()
        } else {
            shortcut_text(&shortcut)
        };
        let label = if recording {
            dc_intl::tr("Stop Recording")
        } else if shortcut.is_empty() {
            dc_intl::tr("Record Keybind")
        } else {
            dc_intl::tr("Edit Keybind")
        };
        let id = keybind.map(|keybind| keybind.id).unwrap_or_default();
        let recorder = div()
            .id("overlay-keybind-recorder")
            .flex_none()
            .h(px(44.))
            .rounded(px(8.))
            .border_1()
            .border_color(if recording {
                color("--text-feedback-critical")
            } else {
                color("--input-border-default")
            })
            .bg(color("--input-background-default"))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.kb_toggle_recording(&id, cx)))
            .child(
                div()
                    .w(px(139.5))
                    .pl(px(10.))
                    .text_size(px(14.))
                    .line_height(px(22.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(if recording { String::new() } else { text }),
            )
            .child(
                div().p(px(4.)).child(
                    div()
                        .h(px(32.))
                        .px(px(11.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-secondary-border-default"))
                        .bg(color("--control-secondary-background-default"))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-secondary-text-default"))
                        .whitespace_nowrap()
                        .child(label),
                ),
            );
        div()
            .py(px(8.))
            .w(px(width))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::tr("Toggle Overlay Lock")),
                    )
                    .when(invalid, |this| {
                        this.child(
                            div()
                                .pt(px(4.))
                                .pb(px(8.))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-feedback-warning"))
                                .child(dc_intl::tr(
                                    "The overlay keybind has been disabled due to an invalid keybind (only shift). Please change the keybind to a different key.",
                                )),
                        )
                    })
                    .when(self.overlay_keybind_admin_warning(), |this| {
                        // Module 119191 (`PIeSHF`): elevated game admin warning.
                        this.child(
                            div()
                                .pt(px(4.))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-feedback-warning"))
                                .child(dc_intl::t("PIeSHF")),
                        )
                    }),
            )
            .child(recorder)
            .into_any_element()
    }

    /// `widgetSettingsContainer_DKMuWq`: the voice widget as the overlay draws it, unlocked, with this account and
    /// three made-up people (`p3`: Acetaminophen server muted, Kovanth speaking, Scynder server deafened), the
    /// "Voice Channel" header and the pin.
    fn overlay_preview(&self) -> AnyElement {
        let settings = self.overlay_settings();
        let layout = self.overlay_layout();
        let made_up = self.images.get(DEFAULT_AVATAR.0).cloned();
        let me = self.stores.current_user.clone().unwrap_or_default();
        let mine = self.images.get(&crate::ui::avatar_source(&me).0).cloned();
        let person = |id: u64, name: String, avatar| Row {
            id,
            name,
            avatar,
            speaking: false,
            mute: false,
            server_mute: false,
            deaf: false,
            server_deaf: false,
            video: false,
            current: id == 1,
        };
        let rows = vec![
            person(
                1,
                me.display_name().to_owned(),
                mine.or_else(|| made_up.clone()),
            ),
            Row {
                server_mute: true,
                ..person(2, dc_intl::tr("Acetaminophen").to_owned(), made_up.clone())
            },
            Row {
                speaking: true,
                ..person(3, dc_intl::tr("Kovanth").to_owned(), made_up.clone())
            },
            Row {
                server_deaf: true,
                ..person(4, dc_intl::tr("Scynder").to_owned(), made_up)
            },
        ];
        // The settings preview (`isSettingsPreview`): the display modes apply as they do while locked.
        let look = RowLook {
            size: settings.avatar_size_mode,
            locked: false,
            preview: true,
            names: settings.display_name_mode,
            flipped: false,
        };
        let list = voice_list(
            &rows,
            &settings,
            layout.voice_max_shown(),
            look,
            &std::collections::HashMap::new(),
            &self.overlay.bounds,
        );
        div()
            .py(px(8.))
            .child(
                div()
                    .h(px(270.))
                    .p(px(32.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-subtle"))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .pl(px(24.))
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap(px(4.))
                            .child(
                                div()
                                    .p(px(12.))
                                    .rounded(px(12.))
                                    .border_1()
                                    .border_dashed()
                                    .border_color(rgba(0xffffff29))
                                    .bg(rgba(0x00000029))
                                    .child(list),
                            )
                            .child(widget_header(dc_intl::tr("Voice Channel").to_owned(), true)),
                    ),
            )
            .into_any_element()
    }
}

/// The open state of the Current Game card, kept with the page's other transient choices.
const CURRENT_GAME_OPEN: &str = "overlay_current_game_open";

/// One switch row of the opened Current Game card (`ps`: `groupContent_DKMuWq` 16/16/12 padding, the text column 2
/// px apart, the switch and the 20 px spacer).
fn game_switch_row(
    title: &'static str,
    line: &'static str,
    hint: Option<&'static str>,
    switch: AnyElement,
) -> AnyElement {
    div()
        .pt(px(16.))
        .px(px(16.))
        .pb(px(12.))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child(line),
                )
                .when_some(hint, |this, hint| {
                    this.child(
                        div()
                            .text_size(px(10.))
                            .line_height(px(12.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(hint),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.))
                .child(switch)
                .child(div().size(px(20.)).flex_none()),
        )
        .into_any_element()
}

impl Root {
    /// `pC` (`OVERLAY_CURRENT_GAME`, module 158935): the running game in a collapsible card (`collapseable_DKMuWq`):
    /// the "Currently Playing" header, the game's name and its overlay status, one switch for the game and a chevron;
    /// opened, the game's own Enable Overlay and Enable Legacy Overlay switches. No capture of this row exists (no
    /// game ran in the reference captures); its geometry is the page's card classes (`group*_DKMuWq`).
    fn overlay_current_game_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let game = self.overlay_current_game()?;
        let game_id = game.id.clone();
        let global = self.overlay_global();
        let enabled = self.overlay_game_enablement(&game);
        let (game_oop, game_legacy) = (enabled.oop, enabled.legacy);
        let (oop, legacy) = (global.oop, global.legacy);
        let method = self.overlay_current_method();
        let rendering = method.is_some_and(OverlayMethod::is_out_of_process);
        let open = self.setting_choices.get(CURRENT_GAME_OPEN) == Some(&Choice::On(true));
        // `P`: the status line; while the overlay draws, the method's name is the `overlayMethodHook` run in
        // `body-sm/medium` and `text-feedback-positive` (`text-feedback-warning` in compatibility mode).
        let plain = |key: &str| vec![(dc_intl::t(key).to_owned(), None)];
        let runs: Vec<(String, Option<Box<str>>)> = if rendering {
            let name = if method == Some(OverlayMethod::OutOfProcessLimitedInteraction) {
                dc_intl::t("506Aba")
            } else {
                dc_intl::t("a3eXSw")
            };
            dc_intl::format_runs("hFVBIg", &[("overlayMethod", name.to_owned().into())])
        } else if !legacy && !oop {
            plain("9DUS5l")
        } else if !game_oop && !game_legacy {
            plain("nQ9EdJ")
        } else if (!game_oop && !legacy && game_legacy) || (!game_legacy && !oop && game_oop) {
            plain("VWUn0a")
        } else {
            // Not the focused window: the native module has not reported the game.
            plain("vwHPRi")
        };
        let method_color = if method == Some(OverlayMethod::OutOfProcessLimitedInteraction) {
            "--text-feedback-warning"
        } else {
            "--text-feedback-positive"
        };
        let header_color = if rendering {
            method_color
        } else {
            "--interactive-text-default"
        };
        let status = div()
            .flex()
            .flex_wrap()
            .children(runs.into_iter().map(|(text, tag)| {
                div()
                    .whitespace_nowrap()
                    .text_color(color(if tag.is_some() {
                        method_color
                    } else {
                        "--text-subtle"
                    }))
                    .when(tag.is_some(), |this| this.font_weight(FontWeight(500.)))
                    .child(text)
            }));
        let checked = (game_oop && oop) || (game_legacy && legacy);
        let name = game.name.clone().unwrap_or_default();
        let toggled = game.clone();
        let main_switch = plain_switch(
            SharedString::from("overlay-current-game"),
            checked,
            move |this, on| this.overlay_current_game_toggled(&toggled, on, checked),
            cx,
        )
        .into_any_element();
        let oop_game = game.clone();
        let legacy_game = game;
        let opened = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .bg(color("--background-base-lower"))
            .child(game_switch_row(
                dc_intl::tr("Enable Overlay"),
                dc_intl::tr(
                    "Lighter, faster, and more customizable. Requires Borderless display setting.",
                ),
                (!oop).then(|| dc_intl::tr("Overlay is disabled for all games.")),
                plain_switch(
                    SharedString::from("overlay-current-game-oop"),
                    game_oop && oop,
                    move |this, on| {
                        if on && !this.overlay_global().oop {
                            this.overlay_set_global(OOP, true);
                        } else {
                            let legacy = this.overlay_game_enablement(&oop_game).legacy;
                            this.detect
                                .store
                                .toggle_overlay(&oop_game, legacy, Some(on));
                            this.persist_detect_store();
                        }
                    },
                    cx,
                )
                .into_any_element(),
            ))
            .child(game_switch_row(
                dc_intl::tr("Enable Legacy Overlay"),
                dc_intl::tr("Legacy Overlay available for all supported games."),
                (!legacy).then(|| dc_intl::tr("Legacy Overlay has been disabled globally")),
                plain_switch(
                    SharedString::from("overlay-current-game-legacy"),
                    game_legacy && legacy,
                    move |this, on| {
                        if on && !this.overlay_global().legacy {
                            this.overlay_set_global(LEGACY, true);
                        } else {
                            let oop = this.overlay_game_enablement(&legacy_game).oop;
                            this.detect
                                .store
                                .toggle_overlay(&legacy_game, on, Some(oop));
                            this.persist_detect_store();
                        }
                    },
                    cx,
                )
                .into_any_element(),
            ));
        let indicator = if rendering {
            svg()
                .path("icons/overlay_screen.svg")
                .size(px(12.))
                .flex_none()
                .text_color(color(header_color))
                .into_any_element()
        } else {
            div()
                .m(px(2.))
                .size(px(8.))
                .rounded_full()
                .bg(color("--text-feedback-positive"))
                .into_any_element()
        };
        let header = div()
            .px(px(16.))
            .py(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .bg(color("--background-base-lowest"))
            .child(indicator)
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(color(header_color))
                    .child(dc_intl::tr("Currently Playing")),
            );
        // `icon`: the game's 40 px icon (`GameIcon` `MEDIUM`, module 769015), the unknown game icon without one.
        let game_icon = match self.overlay_game_icon(game_id.as_deref()) {
            Some(image) => gpui::img(image)
                .size(px(40.))
                .flex_none()
                .rounded(px(8.))
                .into_any_element(),
            None => svg()
                .path("icons/inventory_unknown_game.svg")
                .size(px(40.))
                .flex_none()
                .text_color(color("--interactive-text-default"))
                .into_any_element(),
        };
        if let Some(id) = game_id.clone() {
            let root = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = root.update(cx, |root, _| root.overlay_request_game_icon(&id));
            });
        }
        let content = div()
            .pt(px(16.))
            .px(px(16.))
            .pb(px(12.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .child(game_icon)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(name),
                    )
                    .child(div().text_size(px(14.)).line_height(px(18.)).child(status)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(main_switch)
                    .child(
                        svg()
                            .path(if open {
                                "icons/overlay_chevron_down.svg"
                            } else {
                                "icons/overlay_chevron_right.svg"
                            })
                            .size(px(20.))
                            .flex_none()
                            .text_color(color("--interactive-text-active")),
                    ),
            );
        let card = div()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--background-mod-muted"))
            .bg(color("--background-base-lower"))
            .overflow_hidden()
            .child(
                div()
                    .id("overlay-current-game-card")
                    .when(open, |this| this.bg(color("--background-base-lowest")))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let open =
                            this.setting_choices.get(CURRENT_GAME_OPEN) == Some(&Choice::On(true));
                        this.setting_choices
                            .insert(CURRENT_GAME_OPEN, Choice::On(!open));
                        cx.notify();
                    }))
                    .child(header)
                    .child(content),
            )
            .when(open, |this| this.child(opened));
        Some(card.into_any_element())
    }

    /// The card's own switch, for the game alone, the global switches, or both (`"game"`, `"global"`, `"both"` of
    /// module 158935): turning it on where the game is on but its method is off for all games turns that on.
    fn overlay_current_game_toggled(
        &mut self,
        game: &dc_state::running_games::Game,
        on: bool,
        checked: bool,
    ) {
        use dc_overlay::decision::{SwitchScope, current_game_scope};
        // The current game's switch going off asks why the overlay was turned off (387101 via `pT` of 158935).
        if checked && !on {
            self.overlay_switched_off();
        }
        let scope = current_game_scope(
            on,
            checked,
            self.overlay_global(),
            self.overlay_game_enablement(game),
        );
        if scope != SwitchScope::Game {
            crate::storage::set(
                store::ENABLED_STORE,
                store::enabled_json(dc_overlay::decision::Global {
                    oop: on,
                    legacy: on,
                }),
            );
        }
        if scope != SwitchScope::Global {
            self.detect.store.toggle_overlay(game, on, Some(on));
            self.persist_detect_store();
        }
    }
}

/// "Report a Problem": the card without a border, its Report button primary. The Overlay Issue dialog it opens
/// (module 967055) is not built.
fn overlay_report_card() -> AnyElement {
    card(
        false,
        dc_intl::tr("Report a Problem"),
        dc_intl::tr("If Overlay isn\u{2019}t working as expected, let us know so we can fix it."),
        div()
            .w(px(100.))
            .flex()
            .justify_end()
            .child(button(
                "overlay-report",
                dc_intl::tr("Report"),
                ButtonKind::Primary,
            ))
            .into_any_element(),
    )
}

/// `groupContainer_DKMuWq`: `--background-base-lower`, 8px corners, 16/16/12 padding, 12px between the text and the
/// action; standalone cards have a 1px `--background-mod-muted` border.
fn card(
    standalone: bool,
    title: &'static str,
    line: &'static str,
    action: AnyElement,
) -> AnyElement {
    div()
        .py(px(8.))
        .child(
            div()
                .rounded(px(8.))
                .bg(color("--background-base-lower"))
                .when(standalone, |this| {
                    this.border_1()
                        .border_color(color("--background-mod-muted"))
                })
                .child(
                    div()
                        .pt(px(16.))
                        .px(px(16.))
                        .pb(px(12.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-subtle"))
                                        .child(line),
                                ),
                        )
                        .child(action),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_switch_of_the_page_has_a_store() {
        for key in [
            "overlay_widgets_voice",
            "overlay_widgets_quick_actions",
            "overlay_widgets_notifications",
            "overlay_widgets_chat",
            "overlay_widgets_activity",
            "overlay_widgets_video",
            "overlay_widgets_streams",
        ] {
            assert!(widget_kind(key).is_some(), "{key}");
        }
        for key in [
            "overlay_notifications_text_chat",
            "overlay_notifications_welcome",
            "overlay_notifications_go_live",
            "overlay_notifications_game_activity",
            "overlay_notifications_now_playing",
        ] {
            assert!(notification_setting(key).is_some(), "{key}");
        }
        assert!(widget_kind("overlay_widgets_reset").is_none());
    }

    #[test]
    fn max_users_off_sits_at_the_start() {
        let mut layout = Layout::default();
        assert_eq!(max_users_value(&layout), 8.);
        let mut ids = new_widget_id;
        layout.set_voice_max_shown(0., &mut ids);
        assert_eq!(max_users_value(&layout), 0.);
        layout.set_voice_max_shown(25., &mut ids);
        assert_eq!(max_users_value(&layout), 25.);
    }
}
