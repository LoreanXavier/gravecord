//! Nitro emoji packs (`2026-09-default-emojis`, module `920134` / `934499` / `956396` / `200589`).
//!
//! Renders the `"View Nitro Emojis"` rail trigger (`to`), the `"Nitro Emoji Packs"` catalog panel (`eo`) with
//! collapsible pack sections (`el`), and helpers that integrate pinned packs into the main picker rail, sections,
//! search, and inspector (`VEfeux`).

use super::{Item, Section, SectionIcon, Target};
use crate::net::Request;
use crate::ui::tooltip::{Body, Side, Tip};
use crate::ui::{Root, color, server_landing, theme};
use dc_state::Id;
use dc_state::emoji_packs::{EmojiPack, EmojiPackStore, FetchState, PackEmoji};
use dc_state::premium::PremiumType;
use dc_tokens::Theme;
use gpui::{
    AnyElement, Context, Focusable as _, FontWeight, MouseButton, RenderImage, SharedString, div,
    img, linear_color_stop, linear_gradient, prelude::*, px, rgba, svg,
};
use serde_json::json;
use std::sync::{Arc, OnceLock};

pub(super) const FETCH_KEY: &str = "emoji-packs";
pub(super) const ROUTE: &str = "/emoji-packs";
const PINNED_STORAGE_KEY: &str = "emojiPacks.pinnedIds";
const DISCOVERY_DOT_DISMISSED_KEY: &str = "emojiPacks.discoveryDotDismissed";
const HYDRATED_KEY: &str = "emoji-packs:hydrated";

static BUTTON_GRADIENT_DARK: OnceLock<Arc<RenderImage>> = OnceLock::new();
static BUTTON_GRADIENT_LIGHT: OnceLock<Arc<RenderImage>> = OnceLock::new();
static PANEL_BG_DARK: OnceLock<Arc<RenderImage>> = OnceLock::new();
static PANEL_BG_DARKER: OnceLock<Arc<RenderImage>> = OnceLock::new();
static PANEL_BG_MIDNIGHT: OnceLock<Arc<RenderImage>> = OnceLock::new();
static PANEL_BG_LIGHT: OnceLock<Arc<RenderImage>> = OnceLock::new();

/// Whether the `2026-09-default-emojis` experiment is enabled for the active account (`490035.js`).
pub(super) fn experiment_enabled(root: &Root) -> bool {
    dc_state::emoji_packs::experiment_enabled(
        root.stores
            .experiments
            .variant(dc_state::emoji_packs::EXPERIMENT),
    )
}

/// Whether the picker's intention supports Nitro emoji packs (`200589.js` `z` / `307731.js` `Re`).
pub(super) fn intention_allows_packs(target: Target) -> bool {
    !matches!(
        target,
        Target::GiftConfetti | Target::SoundEmoji | Target::StickerEmoji
    )
}

/// Splits a localized template containing `"{packName}"` around a sentinel so `NitroWheelIcon` can sit inline (`nj` in
/// `920134.js`).
pub(super) fn inspector_pack_parts(pack_name: &str) -> (String, String) {
    const SENTINEL: &str = "\u{1}";
    let formatted = dc_intl::format("VEfeux", &[("packName", SENTINEL.into())]);
    if let Some((prefix, suffix)) = formatted.split_once(SENTINEL) {
        (prefix.to_owned(), format!(" {pack_name}{suffix}"))
    } else {
        (String::new(), format!("{pack_name}: {formatted}"))
    }
}

pub(super) fn read_pinned_pack_ids() -> Vec<String> {
    crate::storage::get(PINNED_STORAGE_KEY)
        .and_then(|value| value.as_array().cloned())
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(str::to_owned))
        .collect()
}

fn write_pinned_pack_ids(ids: &[String]) {
    if crate::storage::settings_writes_blocked() {
        return;
    }
    crate::storage::set(PINNED_STORAGE_KEY, json!(ids));
}

fn discovery_dot_visible() -> bool {
    crate::storage::get(DISCOVERY_DOT_DISMISSED_KEY).and_then(|value| value.as_bool()) != Some(true)
}

fn dismiss_discovery_dot() {
    if !crate::storage::settings_writes_blocked() {
        crate::storage::set(DISCOVERY_DOT_DISMISSED_KEY, json!(true));
    }
}

fn rgba_bytes(token: &str) -> [f32; 4] {
    let rgba = color(token);
    [rgba.r * 255., rgba.g * 255., rgba.b * 255., rgba.a]
}

fn premultiply(c: [f32; 4]) -> [f32; 4] {
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

fn mix_color(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0., 1.);
    std::array::from_fn(|index| a[index] + (b[index] - a[index]) * t)
}

fn source_over_premul(top: [f32; 4], bottom: [f32; 4]) -> [f32; 4] {
    let inv = 1. - top[3];
    std::array::from_fn(|index| top[index] + bottom[index] * inv)
}

fn render_premul_image(
    width: u32,
    height: u32,
    sample: impl Fn(f32, f32) -> [f32; 4],
) -> Arc<RenderImage> {
    let mut pixels = image::RgbaImage::new(width, height);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let u = (x as f32 + 0.5) / width as f32;
        let v = (y as f32 + 0.5) / height as f32;
        let [r, g, b, a] = sample(u, v);
        let channel = |value: f32| (value.clamp(0., 255.) + 0.5) as u8;
        let (r, g, b) = if a > 0. {
            (r / a, g / a, b / a)
        } else {
            (0., 0., 0.)
        };
        // GPUI uploads atlas pixels as straight BGRA inside `image::RgbaImage`.
        *pixel = image::Rgba([channel(b), channel(g), channel(r), channel(a * 255.)]);
    }
    Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]))
}

/// Dual-radial gradient matching `.button_3MXiRB` (`861973`) and `.categoryItemPackCategorySelected_ufks4w` (`148326`).
pub(super) fn button_gradient_image(light: bool) -> Arc<RenderImage> {
    let cache = if light {
        &BUTTON_GRADIENT_LIGHT
    } else {
        &BUTTON_GRADIENT_DARK
    };
    cache
        .get_or_init(|| {
            let start = rgba_bytes(if light {
                "--illo-pink-30"
            } else {
                "--illo-pink-40"
            });
            let end = rgba_bytes(if light {
                "--illo-blue-40"
            } else {
                "--illo-blue-50"
            });
            let base = premultiply(rgba_bytes("--background-mod-strong"));
            let radial_alpha = |dist_norm: f32| {
                if dist_norm <= 0.45 {
                    0.63 + (0.28 - 0.63) * (dist_norm / 0.45)
                } else if dist_norm <= 1.0 {
                    0.28 * (1. - (dist_norm - 0.45) / 0.55)
                } else {
                    0.
                }
            };
            render_premul_image(64, 64, |u, v| {
                let x = u * 32.;
                let y = v * 32.;
                let d_end = ((x - 32. * 0.97).hypot(y - 32. * 0.94)) / 32.;
                let d_start = ((x - 32. * 0.12).hypot(y - 32. * 0.94)) / 32.;
                let end_layer = premultiply([end[0], end[1], end[2], radial_alpha(d_end)]);
                let start_layer =
                    premultiply([start[0], start[1], start[2], radial_alpha(d_start)]);
                source_over_premul(end_layer, source_over_premul(start_layer, base))
            })
        })
        .clone()
}

/// `.background_DERYpk` (`813057`): linear `100.32deg` pink-to-blue gradient fading into `--background-surface-high`.
fn panel_background_image(theme: Theme) -> Arc<RenderImage> {
    let cache = match theme {
        Theme::Light => &PANEL_BG_LIGHT,
        Theme::Dark => &PANEL_BG_DARK,
        Theme::Darker => &PANEL_BG_DARKER,
        Theme::Midnight => &PANEL_BG_MIDNIGHT,
    };
    cache
        .get_or_init(|| {
            let light = theme == Theme::Light;
            let start = rgba_bytes(if light {
                "--illo-pink-30"
            } else {
                "--illo-pink-40"
            });
            let end = rgba_bytes(if light {
                "--illo-blue-40"
            } else {
                "--illo-blue-50"
            });
            let surface = rgba_bytes("--background-surface-high");
            let start_premul = premultiply([start[0], start[1], start[2], 0.60]);
            let end_premul = premultiply([end[0], end[1], end[2], 0.60]);
            let surface_premul = premultiply(surface);
            let angle = 100.32_f32.to_radians();
            // CSS gradient vector points up-right at 100.32deg (clockwise from top).
            let dx = angle.sin();
            let dy = -angle.cos();
            let half_extent = (dx.abs() + dy.abs()) * 0.5;
            render_premul_image(250, 220, |u, v| {
                let proj = ((u - 0.5) * dx + (v - 0.5) * dy + half_extent) / (2. * half_extent);
                let t_lin = ((proj - 0.0167) / (1. - 0.0167)).clamp(0., 1.);
                let lin = mix_color(start_premul, end_premul, t_lin);
                let rad_dist = ((u - 0.5) / 4.11).hypot((v - 1.45) / 1.45);
                let rad_alpha = if rad_dist <= 0.60 {
                    surface_premul
                } else if rad_dist < 1.0 {
                    let fade = 1. - (rad_dist - 0.60) / 0.40;
                    std::array::from_fn(|i| surface_premul[i] * fade)
                } else {
                    [0.; 4]
                };
                let composed =
                    source_over_premul(rad_alpha, source_over_premul(lin, surface_premul));
                std::array::from_fn(|i| composed[i] * 0.5)
            })
        })
        .clone()
}

/// Generates crisp vector-like emoji sprites for headless capture demos when `GRAVECORD_DEMO_EMOJI_PACKS=1` is set.
#[cfg(feature = "capture")]
fn demo_emoji_sprite(kind: u8) -> Arc<gpui::Image> {
    let palette: [[u8; 3]; 14] = [
        [88, 101, 242],  // blurple wave
        [235, 69, 158],  // nitro pink party
        [250, 166, 26],  // gold sparkle
        [237, 66, 69],   // coral heart
        [87, 242, 135],  // mint hype
        [155, 89, 182],  // amethyst sip
        [254, 231, 92],  // crown gold
        [249, 115, 22],  // flame orange
        [56, 189, 248],  // sky gem
        [244, 63, 94],   // rocket crimson
        [250, 204, 21],  // star yellow
        [192, 132, 252], // confetti violet
        [34, 211, 238],  // electric bolt
        [52, 211, 153],  // prism emerald
    ];
    let [r, g, b] = palette[usize::from(kind) % palette.len()];
    let mut pixels = image::RgbaImage::new(64, 64);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let fx = (x as f32 + 0.5) - 32.;
        let fy = (y as f32 + 0.5) - 32.;
        let dist = fx.hypot(fy);
        let radius = 26.0;
        if dist > radius + 1.2 {
            *pixel = image::Rgba([0, 0, 0, 0]);
            continue;
        }
        let edge_alpha = (radius + 1.2 - dist).clamp(0., 1.);
        // Subtle radial highlight + inner motif variation by `kind`.
        let shade = 1.0 - (dist / radius).powf(2.0) * 0.28;
        let highlight = (((fx + 9.).hypot(fy + 9.)) < 9.5) as u8 as f32 * 0.18;
        let left_eye = (fx + 8.5).hypot(fy + 3.5) < 3.4;
        let right_eye = (fx - 8.5).hypot(fy + 3.5) < 3.4;
        let mouth = (fy - 7.5).abs() < 2.2 && fx.abs() < 9.5 && kind.is_multiple_of(2);
        let diamond = (fx.abs() + fy.abs()) < 11.5 && kind % 2 == 1;
        let ring = (dist - 19.5).abs() < 1.3;
        let (cr, cg, cb) =
            if (kind.is_multiple_of(3) && (left_eye || right_eye || mouth)) || diamond {
                (255.0, 255.0, 255.0)
            } else if ring {
                (
                    (r as f32 * 1.15).min(255.),
                    (g as f32 * 1.15).min(255.),
                    (b as f32 * 1.15).min(255.),
                )
            } else {
                (
                    (r as f32 * shade + highlight * 255.).min(255.),
                    (g as f32 * shade + highlight * 255.).min(255.),
                    (b as f32 * shade + highlight * 255.).min(255.),
                )
            };
        *pixel = image::Rgba([cr as u8, cg as u8, cb as u8, (edge_alpha * 255.) as u8]);
    }
    let mut bytes = Vec::new();
    let _ = pixels.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    );
    Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes))
}

#[cfg(feature = "capture")]
fn demo_payload() -> serde_json::Value {
    json!([
        {
            "id": "demo_wumpus_pack",
            "name": "Wumpus & Friends",
            "description": "Official Nitro reaction pack",
            "available": true,
            "representative_emoji_id": "990000000000000101",
            "emojis": [
                { "id": "990000000000000101", "name": "wumpus_wave", "animated": false, "available": true },
                { "id": "990000000000000102", "name": "wumpus_party", "animated": true, "available": true },
                { "id": "990000000000000103", "name": "wumpus_sparkle", "animated": false, "available": true },
                { "id": "990000000000000104", "name": "wumpus_love", "animated": false, "available": true },
                { "id": "990000000000000105", "name": "wumpus_hype", "animated": true, "available": true },
                { "id": "990000000000000106", "name": "wumpus_sip", "animated": false, "available": true },
                { "id": "990000000000000107", "name": "wumpus_crown", "animated": false, "available": true },
                { "id": "990000000000000108", "name": "wumpus_fire", "animated": false, "available": true }
            ]
        },
        {
            "id": "demo_nitro_party",
            "name": "Nitro Party",
            "description": "Animated & celebratory Nitro icons",
            "available": true,
            "representative_emoji_id": "990000000000000201",
            "emojis": [
                { "id": "990000000000000201", "name": "nitro_gem", "animated": false, "available": true },
                { "id": "990000000000000202", "name": "nitro_rocket", "animated": true, "available": true },
                { "id": "990000000000000203", "name": "nitro_star", "animated": false, "available": true },
                { "id": "990000000000000204", "name": "nitro_confetti", "animated": true, "available": true },
                { "id": "990000000000000205", "name": "nitro_bolt", "animated": false, "available": true },
                { "id": "990000000000000206", "name": "nitro_rainbow", "animated": false, "available": true }
            ]
        }
    ])
}

#[cfg(feature = "capture")]
fn demo_mode_enabled() -> bool {
    std::env::var("GRAVECORD_DEMO_EMOJI_PACKS").ok().as_deref() == Some("1")
}

#[cfg(not(feature = "capture"))]
fn demo_mode_enabled() -> bool {
    false
}

pub(super) fn parse_emoji_id(id: &str) -> Option<Id> {
    id.parse::<Id>().ok()
}

pub(super) fn pack_item(emoji: &PackEmoji) -> Option<Item> {
    Some(Item::Custom {
        id: parse_emoji_id(&emoji.id)?,
        name: emoji.name.clone(),
        animated: emoji.animated,
    })
}

pub(super) fn representative_emoji_id(pack: &EmojiPack) -> Option<Id> {
    pack.representative_emoji_id
        .as_deref()
        .and_then(parse_emoji_id)
        .or_else(|| {
            pack.emojis
                .iter()
                .find(|emoji| emoji.available)
                .and_then(|emoji| parse_emoji_id(&emoji.id))
        })
}

pub(super) fn pinned_pack_sections(store: &EmojiPackStore) -> Vec<Section> {
    store
        .pinned_packs()
        .into_iter()
        .filter_map(|pack| {
            let items: Vec<Item> = pack
                .emojis
                .iter()
                .filter(|emoji| emoji.available)
                .filter_map(pack_item)
                .collect();
            if items.is_empty() {
                return None;
            }
            Some(Section {
                key: format!("pack:{}", pack.id),
                label: pack.name.clone(),
                icon: SectionIcon::Pack {
                    id: pack.id.clone(),
                    representative_emoji_id: representative_emoji_id(pack),
                },
                items,
            })
        })
        .collect()
}

fn format_pack_count(count: usize) -> String {
    dc_intl::format("UA8cNe", &[("count", (count as f64).into())])
}

impl Root {
    /// Returns whether Nitro emoji packs should appear in the active picker.
    pub(super) fn nitro_packs_active(&self) -> bool {
        experiment_enabled(self)
            && self
                .picker
                .as_ref()
                .is_none_or(|picker| intention_allows_packs(picker.target))
    }

    /// Reads the live `EmojiPackStore` overlayed with any pending `Request::Rest` result in `self.fetched`.
    pub(super) fn resolved_emoji_packs(&self) -> EmojiPackStore {
        let mut store = self.stores.emoji_packs.clone();
        if !self.requested.contains(HYDRATED_KEY) && store.pinned_pack_ids.is_empty() {
            store.apply_pinned_ids(read_pinned_pack_ids());
        }
        if store.fetch_state != FetchState::Fetched
            && let Some(response) = self.fetched.get(FETCH_KEY)
        {
            store.apply_rest_response(response);
        }
        #[cfg(feature = "capture")]
        if demo_mode_enabled() && store.packs.is_empty() {
            store.apply_packs(&demo_payload());
            if store.pinned_pack_ids.is_empty() {
                let _ = store.pin("demo_wumpus_pack");
            }
        }
        store
    }

    /// Synchronizes stored pins and `GET /emoji-packs` response into `stores.emoji_packs` and requests pack art.
    pub(super) fn sync_emoji_packs(&mut self) {
        if self.requested.insert(HYDRATED_KEY.to_owned())
            && self.stores.emoji_packs.pinned_pack_ids.is_empty()
        {
            self.stores
                .emoji_packs
                .apply_pinned_ids(read_pinned_pack_ids());
        }
        if self.stores.emoji_packs.fetch_state != FetchState::Fetched
            && let Some(response) = self.fetched.get(FETCH_KEY).cloned()
        {
            self.stores.emoji_packs.apply_rest_response(&response);
        }
        #[cfg(feature = "capture")]
        if demo_mode_enabled() && self.stores.emoji_packs.packs.is_empty() {
            self.stores.emoji_packs.apply_packs(&demo_payload());
            if self.stores.emoji_packs.pinned_pack_ids.is_empty() {
                let _ = self.stores.emoji_packs.pin("demo_wumpus_pack");
            }
            for (index, emoji) in self
                .stores
                .emoji_packs
                .packs
                .iter()
                .flat_map(|pack| pack.emojis.iter())
                .enumerate()
            {
                let key = format!("custom:{}", emoji.id);
                self.images
                    .entry(key.clone())
                    .or_insert_with(|| demo_emoji_sprite(index as u8));
                self.requested.insert(key);
            }
        }
        let packs = self.stores.emoji_packs.packs.clone();
        self.request_pack_images(&packs);
    }

    /// Triggers lazy `GET /emoji-packs` when `2026-09-default-emojis` is enabled (`934499.js`).
    pub(super) fn ensure_emoji_packs(&mut self) {
        self.sync_emoji_packs();
        if !experiment_enabled(self) || demo_mode_enabled() {
            return;
        }
        if self.stores.emoji_packs.start_fetch() {
            let _ = self.requests.send(Request::Rest {
                key: Some(FETCH_KEY.to_owned()),
                method: reqwest::Method::GET,
                route: ROUTE.to_owned(),
                body: None,
            });
        }
    }

    fn request_pack_images(&mut self, packs: &[EmojiPack]) {
        for emoji in packs.iter().flat_map(|pack| &pack.emojis) {
            if !emoji.available {
                continue;
            }
            let extension = if emoji.animated { "gif" } else { "webp" };
            let key = format!("custom:{}", emoji.id);
            let url = format!(
                "https://cdn.discordapp.com/emojis/{}.{extension}?size=48",
                emoji.id
            );
            self.request_image(key, url);
        }
    }

    /// Whether an async `GET /emoji-packs` response or demo data has not yet synced into `stores.emoji_packs`.
    pub(super) fn nitro_packs_needs_sync(&self) -> bool {
        if !self.requested.contains(HYDRATED_KEY) && !read_pinned_pack_ids().is_empty() {
            return true;
        }
        if self.stores.emoji_packs.fetch_state != FetchState::Fetched
            && self.fetched.contains_key(FETCH_KEY)
        {
            return true;
        }
        #[cfg(feature = "capture")]
        if demo_mode_enabled() && self.stores.emoji_packs.packs.is_empty() {
            return true;
        }
        false
    }

    pub(super) fn toggle_pin_emoji_pack(
        &mut self,
        pack_id: &str,
        pin: bool,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_emoji_packs();
        let changed = if pin {
            self.stores.emoji_packs.pin(pack_id)
        } else {
            self.stores.emoji_packs.unpin(pack_id)
        };
        if changed {
            write_pinned_pack_ids(&self.stores.emoji_packs.pinned_pack_ids);
            let query = self
                .picker
                .as_ref()
                .map(|picker| picker.search.read(cx).text().to_owned())
                .unwrap_or_default();
            let sections = self.picker_sections("");
            if let Some(picker) = self.picker.as_mut() {
                picker.all = sections;
            }
            self.refresh_picker(&query);
            if !pin && let Some(picker) = self.picker.as_ref() {
                window.focus(&picker.search.read(cx).focus_handle(cx), cx);
            }
        }
        cx.notify();
    }

    pub(super) fn set_show_emoji_packs(
        &mut self,
        show: bool,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_emoji_packs();
        if let Some(picker) = self.picker.as_mut() {
            picker.show_emoji_packs = show;
            if show && picker.expanded_pack_ids.is_empty() {
                if let Some(target_id) = self
                    .stores
                    .emoji_packs
                    .packs
                    .first()
                    .map(|pack| pack.id.clone())
                {
                    picker.expanded_pack_ids.insert(target_id);
                }
            } else if !show {
                window.focus(&picker.search.read(cx).focus_handle(cx), cx);
            }
        }
        if show {
            self.ensure_emoji_packs();
        }
        cx.notify();
    }

    pub(super) fn toggle_pack_expanded(&mut self, pack_id: &str, cx: &mut Context<Self>) {
        if let Some(picker) = self.picker.as_mut()
            && !picker.expanded_pack_ids.remove(pack_id)
        {
            picker.expanded_pack_ids.insert(pack_id.to_owned());
        }
        cx.notify();
    }

    /// Whether a custom emoji in the picker is locked for message insertion (pack emojis require Tier 2).
    pub(super) fn picker_item_locked(&self, id: Id, animated: bool) -> bool {
        if self.nitro_packs_active() {
            let store = self.resolved_emoji_packs();
            let id_str = id.to_string();
            if store.pack_for_emoji(&id_str).is_some() {
                return self.premium() != PremiumType::Nitro;
            }
        }
        self.custom_emoji_locked(id, animated)
    }

    /// `"View Nitro Emojis"` gradient trigger at the top of the category rail (`to` in `920134.js`, `861973`).
    pub(super) fn nitro_packs_rail_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let light = theme() == Theme::Light;
        let label = dc_intl::t("3D0GLv");
        let show_dot = discovery_dot_visible();
        let button = div()
            .id("picker-view-nitro-emojis")
            .size(px(32.))
            .rounded(px(8.))
            .overflow_hidden()
            .relative()
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .hover(|style| style.bg(color("--interactive-background-hover")))
            .on_click(cx.listener(|this, _, window, cx| {
                dismiss_discovery_dot();
                this.set_show_emoji_packs(true, window, cx);
            }))
            .child(
                img(button_gradient_image(light))
                    .id("picker-view-nitro-emojis-bg")
                    .absolute()
                    .inset_0()
                    .size(px(32.)),
            )
            .child(
                svg()
                    .path("icons/sb_plus.svg")
                    .size(px(20.))
                    .text_color(color("--icon-default")),
            );
        let host = div()
            .relative()
            .size(px(32.))
            .child(self.with_tip(
                "picker-view-nitro-emojis",
                Tip::body(Body::Text(label.into()), Side::Right),
                button,
                cx,
            ))
            .when(show_dot, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(-1.))
                        .right(px(-1.))
                        .size(px(10.))
                        .rounded_full()
                        .bg(color("--badge-notification-background"))
                        .border_2()
                        .border_color(color("--background-base-lower")),
                )
            });
        div()
            .w_full()
            .flex()
            .justify_center()
            .pb(px(8.))
            .mb(px(8.))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .child(host)
            .into_any_element()
    }

    /// `"Nitro Emoji Packs"` full-panel catalog (`eo` + `el` in `920134.js`, `813057`, `126904`).
    pub(super) fn nitro_packs_panel(&self, tabless: bool, cx: &mut Context<Self>) -> AnyElement {
        let store = self.resolved_emoji_packs();
        let tier2 = self.premium() == PremiumType::Nitro;
        let light = theme() == Theme::Light;
        let bg_image = panel_background_image(theme());

        let back_label = dc_intl::t("Nwj3fK");
        let back_button = div()
            .id("nitro-packs-back")
            .size(px(28.))
            .rounded(px(6.))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .hover(|style| style.bg(color("--interactive-background-hover")))
            .on_click(cx.listener(|this, _, window, cx| {
                this.set_show_emoji_packs(false, window, cx);
            }))
            .child(
                svg()
                    .path("icons/gif_back.svg")
                    .size(px(16.))
                    .text_color(color("--interactive-text-default")),
            );

        let cta = if tier2 {
            None
        } else {
            let cta_text = dc_intl::t("hIVbiP");
            Some(
                div()
                    .id("nitro-packs-subscribe")
                    .h(px(28.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(rgba(0x8547c6ff), 0.),
                        linear_color_stop(rgba(0xb845c1ff), 1.),
                    ))
                    .hover(|style| style.opacity(0.92))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.picker = None;
                        this.open_nitro(cx);
                    }))
                    .child(
                        svg()
                            .path("icons/nitro_wheel.svg")
                            .size(px(14.))
                            .text_color(gpui::white()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(gpui::white())
                            .child(cta_text),
                    ),
            )
        };

        let header = div()
            .relative()
            .flex_none()
            .pt(px(if tabless { 22. } else { 10. }))
            .px(px(12.))
            .pb(px(10.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .min_w(px(0.))
                    .child(self.with_tip(
                        "nitro-packs-back",
                        Tip::body(Body::Text(back_label.into()), Side::Right),
                        back_button,
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .min_w(px(0.))
                            .child(
                                svg()
                                    .path("icons/nitro_wheel.svg")
                                    .size(px(16.))
                                    .flex_none()
                                    .text_color(color("--text-default")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-default"))
                                            .child(dc_intl::t("0+5+Oy")),
                                    )
                                    .child(server_landing::beta_badge()),
                            ),
                    ),
            )
            .children(cta);

        let mut list_children: Vec<AnyElement> = Vec::new();
        for (pack_index, pack) in store.packs.iter().enumerate() {
            if pack_index > 0 {
                list_children.push(
                    div()
                        .w_full()
                        .h(px(1.))
                        .flex_none()
                        .bg(color("--border-subtle"))
                        .into_any_element(),
                );
            }
            let expanded = self
                .picker
                .as_ref()
                .is_some_and(|picker| picker.expanded_pack_ids.contains(&pack.id));
            list_children.push(self.nitro_pack_section(
                pack_index,
                pack,
                expanded,
                store.is_pinned(&pack.id),
                light,
                cx,
            ));
        }

        let list = div()
            .id("nitro-packs-list")
            .relative()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .pt(px(4.))
            .px(px(12.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .children(list_children);

        div()
            .id("nitro-packs-panel")
            .relative()
            .flex_1()
            .w_full()
            .min_h(px(0.))
            .min_w(px(0.))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                img(bg_image)
                    .id("nitro-packs-background")
                    .absolute()
                    .inset_0()
                    .size_full(),
            )
            .child(header)
            .child(list)
            .into_any_element()
    }

    fn nitro_pack_section(
        &self,
        pack_index: usize,
        pack: &EmojiPack,
        expanded: bool,
        pinned: bool,
        _light: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let available: Vec<&PackEmoji> = pack.emojis.iter().filter(|e| e.available).collect();
        let expand_label = dc_intl::t(if expanded { "iTcuma" } else { "dcl9MQ" });
        let pin_label = dc_intl::t(if pinned { "2zxYCx" } else { "Tvdb5N" });
        let pack_id_toggle = pack.id.clone();
        let pack_id_pin = pack.id.clone();

        let collapse_button = div()
            .id(SharedString::from(format!(
                "nitro-pack-expand-{pack_index}"
            )))
            .size(px(24.))
            .rounded(px(6.))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .hover(|style| style.bg(color("--interactive-background-hover")))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_pack_expanded(&pack_id_toggle, cx);
            }))
            .child(
                svg()
                    .path(if expanded {
                        "icons/overlay_chevron_down.svg"
                    } else {
                        "icons/overlay_chevron_right.svg"
                    })
                    .size(px(14.))
                    .text_color(color("--text-subtle")),
            );

        let pin_button = div()
            .id(SharedString::from(format!("nitro-pack-pin-{pack_index}")))
            .size(px(28.))
            .rounded(px(8.))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .bg(color("--control-secondary-background-default"))
            .border_1()
            .border_color(color("--control-secondary-border-default"))
            .hover(|style| style.bg(color("--control-secondary-background-hover")))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.toggle_pin_emoji_pack(&pack_id_pin, !pinned, window, cx);
            }))
            .child(
                svg()
                    .path(if pinned {
                        "icons/automod_minus.svg"
                    } else {
                        "icons/sb_plus.svg"
                    })
                    .size(px(14.))
                    .text_color(color("--control-secondary-icon-default")),
            );

        let header = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .truncate()
                            .child(pack.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-subtle"))
                            .child(format_pack_count(available.len())),
                    )
                    .child(self.with_tip(
                        SharedString::from(format!("nitro-pack-expand-{pack_index}")),
                        Tip::body(Body::Text(expand_label.into()), Side::Top),
                        collapse_button,
                        cx,
                    )),
            )
            .child(self.with_tip(
                SharedString::from(format!("nitro-pack-pin-{pack_index}")),
                Tip::body(Body::Text(pin_label.into()), Side::Top),
                pin_button,
                cx,
            ));

        let pack_available = pack.available;
        let emoji_buttons = available
            .into_iter()
            .enumerate()
            .filter_map(|(emoji_index, emoji)| {
                let item = pack_item(emoji)?;
                let picture = self.images.get(&format!("custom:{}", emoji.id)).cloned();
                let tip_label = format!(":{}:", emoji.name);
                let cell_id =
                    SharedString::from(format!("nitro-pack-{pack_index}-emoji-{emoji_index}"));
                let button = div()
                    .id(cell_id.clone())
                    .size(px(40.))
                    .rounded(px(8.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(pack_available, |this| {
                        this.cursor_pointer()
                            .hover(|style| style.bg(color("--interactive-background-hover")))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |_, _, _, cx| {
                                    cx.stop_propagation();
                                }),
                            )
                            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.choose_emoji(item.clone(), event.modifiers().shift, cx);
                            }))
                    })
                    .children(picture.map(|picture| {
                        img(picture)
                            .id(SharedString::from(format!(
                                "nitro-pack-{pack_index}-art-{emoji_index}"
                            )))
                            .size(px(32.))
                    }));
                Some(self.with_tip(
                    cell_id,
                    Tip::body(Body::Text(tip_label.into()), Side::Top),
                    button,
                    cx,
                ))
            });

        let emojis = if expanded {
            div()
                .w_full()
                .flex()
                .flex_wrap()
                .gap(px(12.))
                .children(emoji_buttons)
                .into_any_element()
        } else {
            div()
                .id(SharedString::from(format!("nitro-pack-row-{pack_index}")))
                .w_full()
                .h(px(40.))
                .overflow_x_scroll()
                .flex()
                .items_center()
                .gap(px(12.))
                .children(emoji_buttons)
                .into_any_element()
        };

        div()
            .w_full()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(header)
            .child(emojis)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_intl_keys_all_resolve() {
        for key in [
            "3D0GLv", "Nwj3fK", "0+5+Oy", "Tvdb5N", "2zxYCx", "hIVbiP", "iTcuma", "dcl9MQ",
            "leUgZw",
        ] {
            assert!(
                dc_intl::has_key(key),
                "missing dc-intl key {key} needed by Nitro emoji packs"
            );
            assert!(!dc_intl::t(key).is_empty(), "empty translation for {key}");
        }
        assert!(dc_intl::has_key("UA8cNe"));
        assert_eq!(format_pack_count(1), "1 Emoji");
        assert_eq!(format_pack_count(12), "12 Emojis");
        assert!(dc_intl::has_key("VEfeux"));
        let (prefix, suffix) = inspector_pack_parts("Wumpus Wonders");
        assert!(
            prefix.contains("From") && suffix.contains("Wumpus Wonders"),
            "unexpected inspector pack split: ({prefix:?}, {suffix:?})"
        );
    }

    #[test]
    fn pinned_pack_sections_precede_guilds_and_skip_empty_packs() {
        let mut store = EmojiPackStore::default();
        store.apply_packs(&json!([
            {
                "id": "p_good",
                "name": "Wumpus & Friends",
                "available": true,
                "representative_emoji_id": "101",
                "emojis": [
                    { "id": "101", "name": "wave", "animated": false, "available": true },
                    { "id": "102", "name": "party", "animated": true, "available": true }
                ]
            },
            {
                "id": "p_empty",
                "name": "Empty Pack",
                "available": true,
                "emojis": []
            }
        ]));
        store.apply_pinned_ids(vec!["p_good".to_owned(), "p_empty".to_owned()]);
        let sections = pinned_pack_sections(&store);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].key, "pack:p_good");
        assert_eq!(sections[0].label, "Wumpus & Friends");
        assert_eq!(sections[0].items.len(), 2);
        assert!(matches!(
            sections[0].icon,
            SectionIcon::Pack {
                ref id,
                representative_emoji_id: Some(101)
            } if id == "p_good"
        ));
    }

    #[test]
    fn intention_gating_excludes_non_pack_pickers() {
        assert!(intention_allows_packs(Target::Composer));
        assert!(intention_allows_packs(Target::ForwardNote));
        assert!(intention_allows_packs(Target::Field));
        assert!(intention_allows_packs(Target::Reaction {
            channel: 1,
            message: 2
        }));
        assert!(!intention_allows_packs(Target::SoundEmoji));
        assert!(!intention_allows_packs(Target::StickerEmoji));
        assert!(!intention_allows_packs(Target::GiftConfetti));
    }
}
