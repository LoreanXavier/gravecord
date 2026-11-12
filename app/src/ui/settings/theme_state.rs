//! Which theme the app wears, and why: the official `ThemeStore` (module 413599) with its two switches, "Same as
//! Device Theme" and "Sync theme across my devices", and the "Sync with computer" tile.
//!
//! The rules, read in the bundle:
//! - `y()` of `413599`: with `useSystemTheme` on and the device's light or dark known, the palette is the one kept for
//!   that side (`preferences`, light `LIGHT` and dark `DARK` until chosen); otherwise the local appearance settings
//!   when syncing the theme is off (`SelectivelySyncedUserSettingsStore`, `284016`); otherwise the account's
//!   `appearance.theme`.
//! - The colour theme (gradient preset) follows the same order: the one kept for the device side while "Same as
//!   Device Theme" is on (`ClientThemesBackgroundStore.gradientPreset`, `850333`), else the local or account one.
//! - Choosing a default tile (`u_` of `817281`) turns `useSystemTheme` off, ends "Same as Device Theme", keeps the
//!   dark one as the dark side's palette, and writes `appearance` with `theme` set and no colour theme, only when
//!   syncing is on. "Sync with computer" turns `useSystemTheme` on and writes the default theme (`DARK`).
//! - "Same as Device Theme" on (`569386`): syncing appearance turns off (what is shown now stays here), the current
//!   theme becomes its side's choice when no side was chosen before, and the page shows a Light Mode and a Dark Mode
//!   selector (`Z`). Off: `useSystemTheme` and the switch go off; syncing stays off.

use super::model::Choice;
use super::swatches::SWATCHES;
use super::{Root, color};
use crate::net::Request;
use crate::storage;
use dc_proto::settings::CustomTheme;
use dc_tokens::Theme;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, StatefulInteractiveElement, div,
    prelude::*, px,
};
use serde_json::{Value, json};
use std::sync::Mutex;

pub(super) const SAME_AS_DEVICE: &str = "appearance_same_as_device_theme";
pub(super) const SYNC_THEME: &str = "appearance_sync_theme";
const STORAGE_KEY: &str = "settings.theme_state";

/// `Theme` proto values: DARK (Ash) 1, LIGHT 2, DARKER (Dark) 3, MIDNIGHT (Onyx) 4.
pub(super) const ASH: u8 = 1;
pub(super) const LIGHT: u8 = 2;
pub(super) const DARK: u8 = 3;
pub(super) const ONYX: u8 = 4;

/// A theme as the account holds it: the palette's `Theme` value and the colour theme's preset id, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Pick {
    pub theme: u8,
    pub preset: Option<u64>,
}

impl Pick {
    pub(super) fn palette(self) -> Theme {
        palette(self.theme)
    }

    fn json(self) -> Value {
        json!({"theme": self.theme, "preset": self.preset})
    }

    fn from_json(value: &Value) -> Option<Pick> {
        Some(Pick {
            theme: u8::try_from(value["theme"].as_u64()?).ok()?,
            preset: value["preset"].as_u64(),
        })
    }
}

pub(super) fn palette(value: u8) -> Theme {
    match value {
        ASH => Theme::Dark,
        LIGHT => Theme::Light,
        ONYX => Theme::Midnight,
        _ => Theme::Darker,
    }
}

pub(super) fn proto_of(theme: Theme) -> u8 {
    match theme {
        Theme::Dark => ASH,
        Theme::Light => LIGHT,
        Theme::Darker => DARK,
        Theme::Midnight => ONYX,
    }
}

/// `isDark` (module 462887) of a palette: everything but Light.
fn is_dark(theme: u8) -> bool {
    theme != LIGHT
}

/// What this device remembers about the theme: `useSystemTheme`, the palette of each side
/// (`ThemeStore.preferences`), the theme each side keeps while "Same as Device Theme" is on
/// (`syncedClientThemes`), the appearance settings kept here while syncing is off, and the last theme the account
/// held, for a start before the account has answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ThemeState {
    pub use_system: bool,
    pub light: u8,
    pub dark: u8,
    pub synced_light: Option<Pick>,
    pub synced_dark: Option<Pick>,
    pub local: Option<Pick>,
    pub account: Option<Pick>,
    /// `customUserThemeSettings` of the appearance settings kept here while syncing is off, and the last one the
    /// account held (for a start before the account has answered).
    pub local_custom: Option<CustomTheme>,
    pub account_custom: Option<CustomTheme>,
}

fn custom_json(custom: &Option<CustomTheme>) -> Value {
    custom.as_ref().map_or(
        Value::Null,
        |custom| json!({"colors": custom.colors, "angle": custom.angle, "mix": custom.base_mix}),
    )
}

fn custom_from_json(value: &Value) -> Option<CustomTheme> {
    let colors: Vec<String> = value["colors"]
        .as_array()?
        .iter()
        .filter_map(|colour| colour.as_str().map(str::to_owned))
        .collect();
    if colors.is_empty() {
        return None;
    }
    Some(CustomTheme {
        colors,
        angle: u32::try_from(value["angle"].as_u64().unwrap_or(0)).unwrap_or(0),
        base_mix: u32::try_from(value["mix"].as_u64().unwrap_or(0)).unwrap_or(0),
    })
}

impl Default for ThemeState {
    fn default() -> Self {
        ThemeState {
            use_system: false,
            light: LIGHT,
            dark: DARK,
            synced_light: None,
            synced_dark: None,
            local: None,
            account: None,
            local_custom: None,
            account_custom: None,
        }
    }
}

impl ThemeState {
    fn to_json(&self) -> Value {
        let pick = |pick: &Option<Pick>| pick.map_or(Value::Null, Pick::json);
        json!({
            "use_system": self.use_system,
            "light": self.light,
            "dark": self.dark,
            "synced_light": pick(&self.synced_light),
            "synced_dark": pick(&self.synced_dark),
            "local": pick(&self.local),
            "account": pick(&self.account),
            "local_custom": custom_json(&self.local_custom),
            "account_custom": custom_json(&self.account_custom),
        })
    }

    fn from_json(value: &Value) -> ThemeState {
        let fallback = ThemeState::default();
        let number = |key: &str, default: u8| {
            value[key]
                .as_u64()
                .and_then(|v| u8::try_from(v).ok())
                .filter(|v| (1..=4).contains(v))
                .unwrap_or(default)
        };
        ThemeState {
            use_system: value["use_system"].as_bool().unwrap_or(false),
            light: number("light", fallback.light),
            dark: number("dark", fallback.dark),
            synced_light: Pick::from_json(&value["synced_light"]),
            synced_dark: Pick::from_json(&value["synced_dark"]),
            local: Pick::from_json(&value["local"]),
            account: Pick::from_json(&value["account"]),
            local_custom: custom_from_json(&value["local_custom"]),
            account_custom: custom_from_json(&value["account_custom"]),
        }
    }

    fn synced(&self, light: bool) -> Option<Pick> {
        if light {
            self.synced_light
        } else {
            self.synced_dark
        }
    }

    fn set_synced(&mut self, light: bool, pick: Pick) {
        if light {
            self.synced_light = Some(pick);
        } else {
            self.synced_dark = Some(pick);
        }
    }
}

/// The theme that is worn: see the module's rules. `account` is what the account (or, before it answered, the last
/// answer) holds; `device_light` is Windows' choice, `None` when unknown.
pub(super) fn resolve(
    state: &ThemeState,
    same_as_device: bool,
    sync_on: bool,
    device_light: Option<bool>,
    account: Pick,
) -> Pick {
    if state.use_system
        && let Some(light) = device_light
    {
        let theme = if light { state.light } else { state.dark };
        let preset = if same_as_device {
            state.synced(light).and_then(|pick| pick.preset)
        } else {
            None
        };
        return Pick { theme, preset };
    }
    match (sync_on, state.local) {
        (false, Some(local)) => local,
        _ => account,
    }
}

fn cell() -> &'static Mutex<Option<ThemeState>> {
    static STATE: Mutex<Option<ThemeState>> = Mutex::new(None);
    &STATE
}

pub(super) fn state() -> ThemeState {
    let mut guard = cell().lock().unwrap_or_else(|e| e.into_inner());
    guard
        .get_or_insert_with(|| {
            storage::get(STORAGE_KEY)
                .map_or_else(ThemeState::default, |v| ThemeState::from_json(&v))
        })
        .clone()
}

fn store(state: ThemeState) {
    storage::set(STORAGE_KEY, state.to_json());
    *cell().lock().unwrap_or_else(|e| e.into_inner()) = Some(state);
}

fn update(change: impl FnOnce(&mut ThemeState)) {
    let mut next = state();
    change(&mut next);
    store(next);
}

/// What the last frame applied, so a change of any input is applied once.
type Applied = (Pick, bool, Option<CustomTheme>);
static APPLIED: Mutex<Option<Applied>> = Mutex::new(None);

impl Root {
    pub(super) fn theme_same_as_device(&self) -> bool {
        self.pref_on(SAME_AS_DEVICE, false)
    }

    /// `shouldSync("appearance")`: on unless turned off.
    pub(super) fn theme_sync_on(&self) -> bool {
        self.pref_on(SYNC_THEME, true)
    }

    /// The theme the account holds now: `appearance.theme` and the colour theme's preset (Nitro only).
    fn account_pick(&self, state: &ThemeState) -> Pick {
        let theme = match self.stores.theme {
            1..=4 => self.stores.theme as u8,
            _ => state.account.map_or(DARK, |pick| pick.theme),
        };
        let preset = if self.premium() == dc_state::premium::PremiumType::None {
            None
        } else {
            self.stores
                .prefs
                .get("appearance_client_theme_preset")
                .copied()
        };
        Pick { theme, preset }
    }

    /// The theme wanted now.
    pub(super) fn theme_wanted(&self) -> Pick {
        let state = state();
        let mut pick = resolve(
            &state,
            self.theme_same_as_device(),
            self.theme_sync_on(),
            super::controls::device_light(),
            self.account_pick(&state),
        );
        if self.premium() == dc_state::premium::PremiumType::None {
            pick.preset = None;
        }
        pick
    }

    /// Called every frame: puts the palette and the colour theme the rules above give on screen when anything that
    /// decides them changed, and remembers what the account holds for the next start.
    pub(in crate::ui) fn theme_frame(&mut self) {
        if super::client_theme::previewing() {
            return;
        }
        let state = state();
        let account = self.account_pick(&state);
        if self.stores.theme != 0 && state.account != Some(account) {
            update(|next| next.account = Some(account));
        }
        if self.stores.theme != 0 && state.account_custom != self.stores.custom_theme() {
            let held = self.stores.custom_theme();
            update(|next| next.account_custom = held);
        }
        let wanted = self.theme_wanted();
        let custom = self.custom_wanted();
        let key = (
            wanted,
            self.premium() == dc_state::premium::PremiumType::None,
            custom.clone(),
        );
        let mut applied = APPLIED.lock().unwrap_or_else(|e| e.into_inner());
        if applied.as_ref() == Some(&key) {
            return;
        }
        *applied = Some(key);
        drop(applied);
        crate::ui::set_theme(wanted.palette());
        match custom
            .as_ref()
            .and_then(|custom| super::custom_theme::to_editor(custom, wanted.theme))
        {
            Some(custom) => super::client_theme::show_custom(&custom),
            None => super::client_theme::show_preset(wanted.preset),
        }
    }

    /// The custom theme worn now (`customUserThemeSettings` of `SelectivelySyncedUserSettingsStore`): this device's
    /// while syncing is off, the account's otherwise (the last one it held until it has answered). None while "Same as
    /// Device Theme" is on: per-mode theming ignores it (module 590703).
    pub(super) fn custom_wanted(&self) -> Option<CustomTheme> {
        if self.theme_same_as_device() {
            return None;
        }
        let state = state();
        if !self.theme_sync_on() && state.local.is_some() {
            return state.local_custom;
        }
        if self.stores.theme == 0 {
            return state.account_custom;
        }
        self.stores.custom_theme()
    }

    /// "Apply" of the custom theme editor (`G` of module 500779 through `u_`): the theme and the custom theme are
    /// written together, to the account while syncing is on, else kept on this device.
    pub(super) fn theme_apply_custom(&mut self, custom: &CustomTheme, theme: u8) {
        if self.theme_same_as_device() {
            self.set_switch(SAME_AS_DEVICE, false);
        }
        update(|next| {
            next.use_system = false;
            if is_dark(theme) {
                next.dark = theme;
            }
        });
        self.write_custom(custom, theme);
        self.reset_theme_sync();
    }

    fn write_custom(&mut self, custom: &CustomTheme, theme: u8) {
        if self.theme_sync_on() {
            let patch = self.stores.set_custom_theme(u64::from(theme), custom);
            self.send_theme_patch(patch);
        } else {
            update(|next| {
                next.local = Some(Pick {
                    theme,
                    preset: None,
                });
                next.local_custom = Some(custom.clone());
            });
        }
    }

    /// The account's theme is read again at the next frame (READY).
    pub(in crate::ui) fn reset_theme_sync(&mut self) {
        *APPLIED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    fn set_switch(&mut self, key: &'static str, on: bool) {
        self.setting_choices.insert(key, Choice::On(on));
        self.voice_setting_changed(key);
    }

    /// Writes the account's `appearance` the way `u_` does: the theme set and the colour theme set or taken out, in
    /// one PATCH of the whole message. Only while syncing is on; with it off the choice stays in `local`.
    fn write_account_theme(&mut self, pick: Pick) {
        use dc_proto::settings::{CLIENT_THEME_PRESET, CUSTOM_THEME_PREF, THEME};
        // `u_` writes the whole client theme settings: a colour theme or a default theme takes the custom one out.
        self.stores.clear_pref(&CUSTOM_THEME_PREF);
        let theme_value = u64::from(pick.theme);
        match pick.preset {
            Some(id) => {
                self.stores.theme = theme_value;
                self.stores.set_pref(&THEME, theme_value);
                let patch = self.stores.set_pref(&CLIENT_THEME_PRESET, id);
                self.send_theme_patch(patch);
            }
            None => {
                self.stores.clear_pref(&CLIENT_THEME_PRESET);
                self.stores.theme = theme_value;
                let patch = self.stores.set_pref(&THEME, theme_value);
                self.send_theme_patch(patch);
            }
        }
    }

    fn send_theme_patch(&mut self, patch: String) {
        if patch.is_empty() {
            return;
        }
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: "/users/@me/settings-proto/1".to_owned(),
            body: Some(json!({"settings": patch})),
        });
    }

    /// A theme was picked by hand: where it goes (account, or this device while syncing is off) and the screen.
    fn take_theme(&mut self, pick: Pick) {
        if self.theme_sync_on() {
            self.write_account_theme(pick);
        } else {
            update(|next| {
                next.local = Some(pick);
                next.local_custom = None;
            });
        }
    }

    /// A default tile (Light, Ash, Dark, Onyx): `u_({theme})`.
    pub(super) fn theme_choose_default(&mut self, theme: u8) {
        if self.theme_same_as_device() {
            self.set_switch(SAME_AS_DEVICE, false);
        }
        update(|next| {
            next.use_system = false;
            if is_dark(theme) {
                next.dark = theme;
            }
        });
        self.take_theme(Pick {
            theme,
            preset: None,
        });
        self.reset_theme_sync();
    }

    /// The "Sync with computer" tile: `u_({theme: "system"})` writes the default theme and turns the device setting on.
    pub(super) fn theme_choose_system(&mut self) {
        if self.theme_same_as_device() {
            self.set_switch(SAME_AS_DEVICE, false);
        }
        update(|next| next.use_system = true);
        self.take_theme(Pick {
            theme: ASH,
            preset: None,
        });
        self.reset_theme_sync();
    }

    /// A colour theme on a Nitro account: `u_({backgroundGradientPresetId, theme})`.
    pub(super) fn theme_choose_preset(&mut self, pick: Pick) {
        if self.theme_same_as_device() {
            self.set_switch(SAME_AS_DEVICE, false);
        }
        update(|next| next.use_system = false);
        self.take_theme(pick);
        self.reset_theme_sync();
    }

    /// A tile of the Light Mode or Dark Mode selector (`d` of `569386`): the side's palette and colour theme change,
    /// nothing goes to the account.
    fn theme_choose_side(&mut self, light: bool, pick: Pick) {
        update(|next| {
            if light {
                next.light = pick.theme;
            } else {
                next.dark = pick.theme;
            }
            next.set_synced(light, pick);
        });
        self.reset_theme_sync();
    }

    /// The switches changed their value: what follows.
    pub(super) fn theme_switch_changed(&mut self, key: &'static str) {
        if key == SAME_AS_DEVICE {
            if self.theme_same_as_device() {
                self.theme_same_as_device_on();
            } else {
                update(|next| next.use_system = false);
            }
            self.reset_theme_sync();
        } else if key == SYNC_THEME {
            if self.theme_sync_on() {
                // `setShouldSyncAppearanceSettings(true)`: the theme worn now becomes the account's.
                let pick = self.theme_wanted();
                match state().local_custom.clone() {
                    Some(custom) => self.write_custom(&custom, pick.theme),
                    None => self.write_account_theme(pick),
                }
                update(|next| {
                    next.local = None;
                    next.local_custom = None;
                });
            } else {
                let pick = self.theme_wanted();
                let held = self.stores.custom_theme();
                update(|next| {
                    next.local = Some(pick);
                    next.local_custom = held;
                });
            }
            self.reset_theme_sync();
        }
    }

    /// `setValue(true)` of "Same as Device Theme".
    fn theme_same_as_device_on(&mut self) {
        let current = self.theme_wanted();
        // Syncing turns off, keeping what is shown.
        self.setting_choices.insert(SYNC_THEME, Choice::On(false));
        self.voice_setting_changed(SYNC_THEME);
        update(|next| {
            next.local = Some(current);
            if next.synced_light.is_none() && next.synced_dark.is_none() {
                let light = !is_dark(current.theme);
                if light {
                    next.light = current.theme;
                } else {
                    next.dark = current.theme;
                }
                next.set_synced(light, current);
            }
            next.use_system = true;
        });
    }

    /// Rows of the Appearance page that are not always there (`usePredicate`): the colour theme rows give way to the
    /// pair selector while "Same as Device Theme" is on, unless the account has no Nitro.
    pub(super) fn appearance_row_shown(&self, key: &str) -> bool {
        match key {
            "appearance_custom_themes_upsell" | "appearance_client_themes" => {
                !self.theme_same_as_device()
                    || self.premium() == dc_state::premium::PremiumType::None
            }
            _ => true,
        }
    }

    /// `useDisabled` of "Sync theme across my devices": locked while "Same as Device Theme" is on.
    pub(super) fn appearance_switch_disabled(&self, key: &str) -> bool {
        key == SYNC_THEME && self.theme_same_as_device()
    }

    /// The selected state of the default tiles (`569386`/`270746`): a palette tile when no colour theme is worn and
    /// the device setting is off, the sync tile when it is on.
    pub(super) fn theme_tile_selected(&self, name: &str) -> bool {
        let state = state();
        let wanted = self.theme_wanted();
        if name == "Sync with computer" {
            return state.use_system && wanted.preset.is_none() && !self.theme_same_as_device();
        }
        if state.use_system || wanted.preset.is_some() {
            return false;
        }
        let value = match name {
            "Light" => LIGHT,
            "Ash" => ASH,
            "Dark" => DARK,
            "Onyx" => ONYX,
            _ => return false,
        };
        wanted.theme == value
    }

    /// `Z`: the Light Mode and Dark Mode selectors, side by side (`container_MCHqk6`: two columns, 24px apart, 16px
    /// above), each a heading and its tiles 8px apart, 16px under it.
    pub(super) fn dual_theme_selector(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = state();
        let nitro = self.premium() != dc_state::premium::PremiumType::None;
        let side = |light: bool, heading: &'static str, cx: &mut Context<Self>| {
            let chosen = if light { state.light } else { state.dark };
            let preset = state.synced(light).and_then(|pick| pick.preset);
            let defaults: &[(&str, u8)] = if light {
                &[("Light", LIGHT)]
            } else {
                &[("Ash", ASH), ("Dark", DARK), ("Onyx", ONYX)]
            };
            let mut tiles: Vec<AnyElement> = Vec::new();
            for (name, theme) in defaults {
                let Some(swatch) = SWATCHES.iter().find(|swatch| swatch.name == *name) else {
                    continue;
                };
                let selected = (!nitro || preset.is_none()) && chosen == *theme;
                let theme = *theme;
                tiles.push(self.swatch_tile(
                    swatch,
                    selected,
                    cx.listener(move |this, _, _, cx| {
                        this.theme_choose_side(
                            light,
                            Pick {
                                theme,
                                preset: None,
                            },
                        );
                        cx.notify();
                    }),
                ));
            }
            if nitro {
                for swatch in SWATCHES
                    .iter()
                    .filter(|swatch| swatch.group == "client" && !swatch.layers.is_empty())
                    .filter(|swatch| super::client_theme::is_light(swatch) == light)
                {
                    let Some(id) = super::client_theme::preset_id(swatch.name) else {
                        continue;
                    };
                    let theme = super::client_theme::base_of(swatch);
                    let theme = proto_of(theme);
                    tiles.push(self.swatch_tile(
                        swatch,
                        preset == Some(id),
                        cx.listener(move |this, _, _, cx| {
                            this.theme_choose_side(
                                light,
                                Pick {
                                    theme,
                                    preset: Some(id),
                                },
                            );
                            cx.notify();
                        }),
                    ));
                }
            }
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(heading),
                )
                .child(
                    div()
                        .pt(px(16.))
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .children(tiles),
                )
        };
        div()
            .id(SharedString::from("appearance-dual-theme"))
            .mt(px(16.))
            .grid()
            .grid_cols(2)
            .gap(px(24.))
            .items_start()
            .child(side(true, "Light Mode", cx))
            .child(side(false, "Dark Mode", cx))
            .into_any_element()
    }

    /// A 48px tile with the click handler of the caller; the chosen one carries its ring and tick.
    fn swatch_tile(
        &self,
        swatch: &'static super::model::Swatch,
        selected: bool,
        on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> AnyElement {
        self.swatch_base(swatch, selected)
            .on_click(on_click)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(theme: u8) -> Pick {
        Pick {
            theme,
            preset: None,
        }
    }

    #[test]
    fn the_account_decides_without_the_device_setting() {
        let state = ThemeState::default();
        assert_eq!(
            resolve(&state, false, true, Some(true), account(DARK)),
            account(DARK)
        );
    }

    #[test]
    fn the_device_side_decides_with_it() {
        let state = ThemeState {
            use_system: true,
            light: LIGHT,
            dark: ONYX,
            ..ThemeState::default()
        };
        assert_eq!(
            resolve(&state, true, false, Some(true), account(DARK)).theme,
            LIGHT
        );
        assert_eq!(
            resolve(&state, true, false, Some(false), account(DARK)).theme,
            ONYX
        );
        // The device's choice is not known: the account's theme stands.
        assert_eq!(
            resolve(&state, true, false, None, account(DARK)).theme,
            DARK
        );
    }

    #[test]
    fn a_colour_theme_follows_the_device_side_only_in_the_pair_mode() {
        let state = ThemeState {
            use_system: true,
            synced_dark: Some(Pick {
                theme: DARK,
                preset: Some(8),
            }),
            ..ThemeState::default()
        };
        assert_eq!(
            resolve(&state, true, false, Some(false), account(DARK)).preset,
            Some(8)
        );
        assert_eq!(
            resolve(&state, false, false, Some(false), account(DARK)).preset,
            None
        );
    }

    #[test]
    fn with_syncing_off_the_local_theme_wins_over_the_account() {
        let state = ThemeState {
            local: Some(account(LIGHT)),
            ..ThemeState::default()
        };
        assert_eq!(
            resolve(&state, false, false, None, account(DARK)).theme,
            LIGHT
        );
        assert_eq!(
            resolve(&state, false, true, None, account(DARK)).theme,
            DARK
        );
    }

    #[test]
    fn the_state_survives_a_restart() {
        let state = ThemeState {
            use_system: true,
            light: LIGHT,
            dark: ASH,
            synced_light: Some(account(LIGHT)),
            synced_dark: Some(Pick {
                theme: DARK,
                preset: Some(12),
            }),
            local: Some(account(ONYX)),
            account: Some(Pick {
                theme: DARK,
                preset: Some(3),
            }),
            local_custom: Some(CustomTheme {
                colors: vec!["#5865f2".to_owned(), "#ff0000".to_owned()],
                angle: 90,
                base_mix: 74,
            }),
            account_custom: None,
        };
        assert_eq!(ThemeState::from_json(&state.to_json()), state);
        assert_eq!(ThemeState::from_json(&json!({})), ThemeState::default());
    }

    #[test]
    fn palettes_and_proto_values_are_the_same_numbers_both_ways() {
        for value in [ASH, LIGHT, DARK, ONYX] {
            assert_eq!(proto_of(palette(value)), value);
        }
    }
}
