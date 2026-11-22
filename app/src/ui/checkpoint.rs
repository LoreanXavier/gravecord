//! Checkpoint 2026 full-screen interactive year-in-review overlay (`Checkpoint2026`, module 925433).
//!
//! Renders the dark `#000000` retro cyan (`#34E2F1`/`#1482A7`) multi-slide experience:
//! Home, Voice, Messages, Servers, Emoji, Games, Squad/Sidekick (with inline DM composer
//! `0ZQw/X` / `oeb1vg` / `fjcCk5` and Gift Nitro `Ve9Ge6`), Character builder (7 traits with
//! Nitro-lock guard), Summary, and Share cards, with top-right Mute (`w4m945`/`YqAjXy`) +
//! Close (`cpT0Cq`), bottom step progress bar, and Back/Next/Save/Share/Done navigation.

use super::{Root, color, icon};
use crate::net::Request;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
};
use serde_json::json;

/// Official Checkpoint 2026 theme constants (`532294`).
const CYAN_PRIMARY: u32 = 0x34e2f1ff;
const CYAN_DARK: u32 = 0x1482a7ff;
const CYAN_CARD_BG: u32 = 0x04363abf;
const CHECKPOINT_WHITE: u32 = 0xdfdfdfff;

/// Ordered Checkpoint 2026 slides (`201779.Np`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckpointSlide {
    Home,
    Voice,
    Messages,
    Servers,
    Emoji,
    Games,
    Squad,
    Character,
    Summary,
    Share,
}

impl CheckpointSlide {
    pub(crate) const ALL: [Self; 10] = [
        Self::Home,
        Self::Voice,
        Self::Messages,
        Self::Servers,
        Self::Emoji,
        Self::Games,
        Self::Squad,
        Self::Character,
        Self::Summary,
        Self::Share,
    ];

    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|&s| s == self).unwrap_or(0)
    }

    pub(crate) fn next(self) -> Option<Self> {
        Self::ALL.get(self.index() + 1).copied()
    }

    pub(crate) fn prev(self) -> Option<Self> {
        self.index()
            .checked_sub(1)
            .and_then(|i| Self::ALL.get(i).copied())
    }
}

/// Share card variants (`796104.jA`: `DEFAULT` | `CHARACTER_ONLY` | `STATS_ONLY`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CheckpointCardType {
    #[default]
    Default,
    CharacterOnly,
    StatsOnly,
}

/// Character builder trait selection (`AB_Master` / `SM_Master`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CheckpointTraits {
    pub face: String,
    pub outfit: String,
    pub outfit_color: String,
    pub hat: String,
    pub shoes: String,
    pub wearable: String,
    pub aura: String,
    pub nitro_locked_selected: bool,
}

impl Default for CheckpointTraits {
    fn default() -> Self {
        Self {
            face: "Classic".into(),
            outfit: "Cyber Hoodie".into(),
            outfit_color: "Cyan".into(),
            hat: "None".into(),
            shoes: "Neon Runners".into(),
            wearable: "Headset".into(),
            aura: "Pulse".into(),
            nitro_locked_selected: false,
        }
    }
}

/// User stats rendered across Checkpoint 2026 slides.
#[derive(Clone, Debug)]
pub(crate) struct CheckpointStats {
    pub username: String,
    pub voice_minutes: u64,
    pub messages_sent: u64,
    pub servers_visited: u32,
    pub top_server: String,
    pub favorite_emoji: String,
    pub games_played: u32,
    pub top_game: String,
    pub sidekick_name: String,
    pub shared_data: bool,
}

impl Default for CheckpointStats {
    fn default() -> Self {
        Self {
            username: "LoreanXavier".into(),
            voice_minutes: 4_820,
            messages_sent: 18_430,
            servers_visited: 24,
            top_server: "gravecord HQ".into(),
            favorite_emoji: "🔥".into(),
            games_played: 19,
            top_game: "Elden Ring".into(),
            sidekick_name: "lawleis".into(),
            shared_data: true,
        }
    }
}

/// Active state for module 925433 (`Checkpoint2026`).
#[derive(Clone, Debug)]
pub(crate) struct CheckpointState {
    pub slide: CheckpointSlide,
    pub muted: bool,
    pub saving: bool,
    pub sidekick_dm_sent: bool,
    pub share_forwarded: bool,
    pub card_type: CheckpointCardType,
    pub stats: CheckpointStats,
    pub traits: CheckpointTraits,
    pub has_nitro: bool,
}

impl Default for CheckpointState {
    fn default() -> Self {
        Self {
            slide: CheckpointSlide::Home,
            muted: false,
            saving: false,
            sidekick_dm_sent: false,
            share_forwarded: false,
            card_type: CheckpointCardType::Default,
            stats: CheckpointStats::default(),
            traits: CheckpointTraits::default(),
            has_nitro: true,
        }
    }
}

fn cp_copy(key: &str, fallback: &str) -> String {
    if dc_intl::has_key(key) {
        let s = dc_intl::t(key);
        if !s.trim().is_empty() && s != key {
            return s.to_owned();
        }
    }
    fallback.to_owned()
}

/// Module 925433 / 796104 (`GET /checkpoint`).
#[allow(dead_code)]
pub(crate) fn fetch_checkpoint_request() -> Request {
    Request::Rest {
        key: Some("checkpoint:get".to_owned()),
        method: reqwest::Method::GET,
        route: "/checkpoint".to_owned(),
        body: None,
    }
}

/// Module 925433 / 796104 (`POST /checkpoint/complete`).
pub(crate) fn complete_checkpoint_request(traits: &CheckpointTraits) -> Request {
    Request::Rest {
        key: Some("checkpoint:complete".to_owned()),
        method: reqwest::Method::POST,
        route: "/checkpoint/complete".to_owned(),
        body: Some(json!({
            "face": traits.face,
            "outfit": traits.outfit,
            "outfit_color": traits.outfit_color,
            "hat": traits.hat,
            "shoes": traits.shoes,
            "wearable": traits.wearable,
            "aura": traits.aura,
        })),
    }
}

/// Module 925433 / 796104 (`DELETE /checkpoint/reset`).
#[allow(dead_code)]
pub(crate) fn reset_checkpoint_request() -> Request {
    Request::Rest {
        key: Some("checkpoint:reset".to_owned()),
        method: reqwest::Method::DELETE,
        route: "/checkpoint/reset".to_owned(),
        body: None,
    }
}

impl Root {
    #[allow(dead_code)]
    pub(crate) fn open_checkpoint_2026(&mut self, cx: &mut Context<Self>) {
        let username = self
            .stores
            .current_user
            .as_ref()
            .map(|u| u.username.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Explorer".to_owned());
        let has_nitro = self.stores.account.premium_type.is_some_and(|p| p > 0);
        let mut state = CheckpointState {
            has_nitro,
            ..Default::default()
        };
        state.stats.username = username;
        self.checkpoint_2026 = Some(state);
        let _ = self.requests.send(fetch_checkpoint_request());
        cx.notify();
    }

    pub(crate) fn close_checkpoint_2026(&mut self) -> bool {
        self.checkpoint_2026.take().is_some()
    }

    pub(in crate::ui) fn checkpoint_2026_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.checkpoint_2026.as_ref()?;
        let slide = state.slide;
        let idx = slide.index();
        let blocked_trait = state.traits.nitro_locked_selected && !state.has_nitro;

        let stat_card = |label: String, value: String, subtitle: String| {
            div()
                .w(px(540.))
                .p(px(28.))
                .rounded(px(16.))
                .bg(rgba(CYAN_CARD_BG))
                .border_2()
                .border_color(rgba(CYAN_PRIMARY))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight(700.))
                        .text_color(rgba(CYAN_PRIMARY))
                        .child(label.to_uppercase()),
                )
                .child(
                    div()
                        .text_size(px(48.))
                        .line_height(px(54.))
                        .font_weight(FontWeight(800.))
                        .text_color(rgba(CHECKPOINT_WHITE))
                        .child(value),
                )
                .child(
                    div()
                        .text_size(px(15.))
                        .text_center()
                        .text_color(rgba(0xdfdfdfcc))
                        .child(subtitle),
                )
        };

        let center_stage: AnyElement = match slide {
            CheckpointSlide::Home => div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(14.))
                .child(
                    div()
                        .px(px(12.))
                        .py(px(4.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgba(CYAN_PRIMARY))
                        .text_size(px(12.))
                        .font_weight(FontWeight(700.))
                        .text_color(rgba(CYAN_PRIMARY))
                        .child(cp_copy("CdU/PF", "CHECKPOINT 2026")),
                )
                .child(
                    div()
                        .text_size(px(44.))
                        .line_height(px(50.))
                        .font_weight(FontWeight(800.))
                        .text_color(rgba(CHECKPOINT_WHITE))
                        .child(cp_copy("ir6nEQ", "Your Checkpoint")),
                )
                .child(div().text_size(px(16.)).text_color(rgba(0xdfdfdfcc)).child(
                    if state.stats.shared_data {
                        format!(
                            "@{} — {}",
                            state.stats.username,
                            cp_copy("ZQakPR", "Your Discord")
                        )
                    } else {
                        cp_copy("ZQakPR", "Your Discord")
                    },
                ))
                .into_any_element(),
            CheckpointSlide::Voice => stat_card(
                dc_intl::t("K3lovD").to_owned(),
                format!("{} min", state.stats.voice_minutes),
                cp_copy("ZQakPR", "Your Discord"),
            )
            .into_any_element(),
            CheckpointSlide::Messages => stat_card(
                dc_intl::t("OIgYlQ").to_owned(),
                state.stats.messages_sent.to_string(),
                cp_copy("wWKySP", "Messages sent"),
            )
            .into_any_element(),
            CheckpointSlide::Servers => stat_card(
                dc_intl::t("7hB4kg").to_owned(),
                state.stats.top_server.clone(),
                format!(
                    "{} • {} {}",
                    cp_copy("8XPMJj", "Top Server"),
                    state.stats.servers_visited,
                    dc_intl::t("7hB4kg")
                ),
            )
            .into_any_element(),
            CheckpointSlide::Emoji => stat_card(
                dc_intl::t("sMOuuS").to_owned(),
                state.stats.favorite_emoji.clone(),
                cp_copy("3sARW7", "Favorite Emoji"),
            )
            .into_any_element(),
            CheckpointSlide::Games => stat_card(
                dc_intl::t("URyqtP").to_owned(),
                state.stats.top_game.clone(),
                format!(
                    "{} • {} {}",
                    cp_copy("RbT4Zq", "Most Played Game"),
                    state.stats.games_played,
                    dc_intl::t("URyqtP")
                ),
            )
            .into_any_element(),
            CheckpointSlide::Squad => {
                let dm_label = dc_intl::format(
                    "0ZQw/X",
                    &[("name", state.stats.sidekick_name.as_str().into())],
                );
                let sent = state.sidekick_dm_sent;
                div()
                    .w(px(540.))
                    .p(px(28.))
                    .rounded(px(16.))
                    .bg(rgba(CYAN_CARD_BG))
                    .border_2()
                    .border_color(rgba(CYAN_PRIMARY))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight(700.))
                            .text_color(rgba(CYAN_PRIMARY))
                            .child(format!(
                                "{} • {}",
                                cp_copy("OzY5Ns", "Squad"),
                                cp_copy("+uwOHt", "Sidekick")
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(36.))
                            .font_weight(FontWeight(800.))
                            .text_color(rgba(CHECKPOINT_WHITE))
                            .child(format!("@{}", state.stats.sidekick_name)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .pt(px(6.))
                            .child(
                                div()
                                    .id("checkpoint-sidekick-dm")
                                    .h(px(38.))
                                    .px(px(14.))
                                    .rounded(px(8.))
                                    .bg(rgba(CYAN_PRIMARY))
                                    .when(sent, |d| d.opacity(0.6))
                                    .when(!sent, |d| d.cursor_pointer())
                                    .flex()
                                    .items_center()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(rgba(0x000000ff))
                                    .child(if sent {
                                        dc_intl::t("fjcCk5").to_owned()
                                    } else if !dm_label.is_empty() {
                                        dm_label
                                    } else {
                                        dc_intl::t("oeb1vg").to_owned()
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(cp) = this.checkpoint_2026.as_mut() {
                                            cp.sidekick_dm_sent = true;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("checkpoint-gift-nitro")
                                    .h(px(38.))
                                    .px(px(14.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(rgba(CYAN_PRIMARY))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(icon("nitro_wheel", 15., "--white"))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(rgba(CHECKPOINT_WHITE))
                                            .child(dc_intl::t("Ve9Ge6")),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_checkpoint_2026();
                                        this.open_nitro(cx);
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            CheckpointSlide::Character => {
                let outfits = ["Cyber Hoodie", "Retro Jacket", "None"];
                let active_outfit = state.traits.outfit.clone();
                div()
                    .w(px(560.))
                    .p(px(24.))
                    .rounded(px(16.))
                    .bg(rgba(CYAN_CARD_BG))
                    .border_2()
                    .border_color(rgba(CYAN_PRIMARY))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight(700.))
                            .text_color(rgba(CYAN_PRIMARY))
                            .child(cp_copy("CdU/PF", "Checkpoint").to_uppercase()),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(rgba(CHECKPOINT_WHITE))
                            .child(format!(
                                "{} • {} • {}",
                                state.traits.face, state.traits.outfit, state.traits.aura
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .children(outfits.into_iter().map(|opt| {
                                let selected = active_outfit == opt;
                                let opt_str = opt.to_owned();
                                div()
                                    .id(SharedString::from(format!("cp-outfit-{opt}")))
                                    .h(px(34.))
                                    .px(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(if selected {
                                        rgba(CYAN_PRIMARY)
                                    } else {
                                        rgba(CYAN_DARK)
                                    })
                                    .bg(if selected {
                                        rgba(0x34e2f133)
                                    } else {
                                        rgba(0x00000055)
                                    })
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(rgba(CHECKPOINT_WHITE))
                                    .child(if opt == "None" {
                                        dc_intl::t("PoWNfe").to_owned()
                                    } else {
                                        opt_str.clone()
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(cp) = this.checkpoint_2026.as_mut() {
                                            cp.traits.outfit = opt_str.clone();
                                        }
                                        cx.notify();
                                    }))
                            })),
                    )
                    .into_any_element()
            }
            CheckpointSlide::Summary => div()
                .w(px(620.))
                .p(px(28.))
                .rounded(px(16.))
                .bg(rgba(CYAN_CARD_BG))
                .border_2()
                .border_color(rgba(CYAN_PRIMARY))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(16.))
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight(800.))
                        .text_color(rgba(CYAN_PRIMARY))
                        .child(cp_copy("y4M1na", "SUMMARY").to_uppercase()),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_wrap()
                        .justify_center()
                        .gap(px(16.))
                        .child(summary_stat_tile(
                            dc_intl::t("OIgYlQ"),
                            state.stats.messages_sent.to_string(),
                        ))
                        .child(summary_stat_tile(
                            dc_intl::t("K3lovD"),
                            format!("{}m", state.stats.voice_minutes),
                        ))
                        .child(summary_stat_tile(
                            dc_intl::t("7hB4kg"),
                            state.stats.servers_visited.to_string(),
                        ))
                        .child(summary_stat_tile(
                            dc_intl::t("URyqtP"),
                            state.stats.games_played.to_string(),
                        )),
                )
                .into_any_element(),
            CheckpointSlide::Share => {
                let forwarded = state.share_forwarded;
                let active_card = state.card_type;
                let card_opts = [
                    (CheckpointCardType::Default, "Default"),
                    (CheckpointCardType::CharacterOnly, "Character"),
                    (CheckpointCardType::StatsOnly, "Stats"),
                ];
                div()
                    .w(px(540.))
                    .p(px(28.))
                    .rounded(px(16.))
                    .bg(rgba(CYAN_CARD_BG))
                    .border_2()
                    .border_color(rgba(CYAN_PRIMARY))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight(800.))
                            .text_color(rgba(CHECKPOINT_WHITE))
                            .child(dc_intl::t("RDE0Sc")),
                    )
                    .child(div().flex().gap(px(8.)).children(card_opts.into_iter().map(
                        |(ct, lbl)| {
                            let sel = active_card == ct;
                            div()
                                .id(SharedString::from(format!("cp-share-card-{lbl}")))
                                .h(px(30.))
                                .px(px(10.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(if sel {
                                    rgba(CYAN_PRIMARY)
                                } else {
                                    rgba(CYAN_DARK)
                                })
                                .bg(if sel {
                                    rgba(0x34e2f133)
                                } else {
                                    rgba(0x00000055)
                                })
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(12.))
                                .font_weight(FontWeight(600.))
                                .text_color(rgba(CHECKPOINT_WHITE))
                                .child(dc_intl::tr(lbl))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(cp) = this.checkpoint_2026.as_mut() {
                                        cp.card_type = ct;
                                    }
                                    cx.notify();
                                }))
                        },
                    )))
                    .child(
                        div()
                            .id("checkpoint-forward-card")
                            .h(px(40.))
                            .px(px(18.))
                            .rounded(px(8.))
                            .bg(rgba(CYAN_PRIMARY))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(icon("clips_share", 16., "--black"))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(rgba(0x000000ff))
                                    .child(if forwarded {
                                        dc_intl::t("kwmYkt")
                                    } else {
                                        dc_intl::t("I3ltXO")
                                    }),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(cp) = this.checkpoint_2026.as_mut() {
                                    cp.share_forwarded = true;
                                }
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
        };

        let primary_label = match slide {
            CheckpointSlide::Home => dc_intl::t("I0v0Qv"),
            CheckpointSlide::Character => dc_intl::t("R3BPH+"),
            CheckpointSlide::Summary => dc_intl::t("RDE0Sc"),
            CheckpointSlide::Share => dc_intl::t("i4jeWR"),
            _ => dc_intl::t("PDTjLN"),
        };

        Some(
            deferred(
                div()
                    .id("checkpoint-2026-overlay")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000ff))
                    .child(div().absolute().inset_0().bg(linear_gradient(
                        180.,
                        linear_color_stop(rgba(0x1482a744), 0.),
                        linear_color_stop(rgba(0x000000ff), 0.75),
                    )))
                    // Top-right Mute (`w4m945`/`YqAjXy`) + Close (`cpT0Cq`)
                    .child(
                        div()
                            .absolute()
                            .top(px(28.))
                            .right(px(28.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .id("checkpoint-mute-btn")
                                    .size(px(40.))
                                    .rounded(px(8.))
                                    .border_2()
                                    .border_color(rgba(CYAN_PRIMARY))
                                    .bg(rgba(0x04363aff))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .aria_label(SharedString::from(if state.muted {
                                        dc_intl::t("YqAjXy")
                                    } else {
                                        dc_intl::t("w4m945")
                                    }))
                                    .child(icon(
                                        if state.muted {
                                            "sound_no_emoji"
                                        } else {
                                            "sound_speaker"
                                        },
                                        18.,
                                        "--white",
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(cp) = this.checkpoint_2026.as_mut() {
                                            cp.muted = !cp.muted;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("checkpoint-close-btn")
                                    .size(px(40.))
                                    .rounded(px(8.))
                                    .border_2()
                                    .border_color(rgba(CYAN_PRIMARY))
                                    .bg(rgba(0x04363aff))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .aria_label(SharedString::from(dc_intl::t("cpT0Cq")))
                                    .child(icon("modal_close", 18., "--white"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_checkpoint_2026();
                                        cx.notify();
                                    })),
                            ),
                    )
                    // Center slide stage
                    .child(
                        div()
                            .relative()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .p(px(48.))
                            .child(center_stage),
                    )
                    // Bottom navigation bar + step dots (`428692` / `753875`)
                    .child(
                        div()
                            .absolute()
                            .bottom(px(28.))
                            .left(px(32.))
                            .right(px(32.))
                            .h(px(48.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(if slide.prev().is_some() {
                                div()
                                    .id("checkpoint-back-btn")
                                    .h(px(44.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .border_2()
                                    .border_color(rgba(CYAN_PRIMARY))
                                    .bg(rgba(0x04363aff))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(icon("clips_back", 16., "--white"))
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(rgba(CHECKPOINT_WHITE))
                                            .child(dc_intl::t("13/7kX")),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(cp) = this.checkpoint_2026.as_mut()
                                            && let Some(prev) = cp.slide.prev()
                                        {
                                            cp.slide = prev;
                                        }
                                        cx.notify();
                                    }))
                                    .into_any_element()
                            } else {
                                div().w(px(96.)).into_any_element()
                            })
                            // Step progress dots
                            .child(div().flex().items_center().gap(px(8.)).children(
                                CheckpointSlide::ALL.iter().enumerate().map(|(i, &s)| {
                                    let active = i == idx;
                                    let passed = i < idx;
                                    div()
                                        .id(SharedString::from(format!("checkpoint-dot-{i}")))
                                        .size(px(18.))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .size(if active { px(10.) } else { px(7.) })
                                                .rounded_full()
                                                .bg(if active {
                                                    rgba(CYAN_PRIMARY)
                                                } else if passed {
                                                    rgba(CYAN_DARK)
                                                } else {
                                                    rgba(0xffffff44)
                                                }),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(cp) = this.checkpoint_2026.as_mut() {
                                                cp.slide = s;
                                            }
                                            cx.notify();
                                        }))
                                }),
                            ))
                            // Primary Next / Start / Save / Share / Done button
                            .child(
                                div()
                                    .id("checkpoint-next-btn")
                                    .h(px(44.))
                                    .px(px(18.))
                                    .rounded(px(8.))
                                    .border_2()
                                    .border_color(rgba(CYAN_PRIMARY))
                                    .bg(rgba(CYAN_PRIMARY))
                                    .when(blocked_trait || state.saving, |d| d.opacity(0.5))
                                    .when(!blocked_trait && !state.saving, |d| d.cursor_pointer())
                                    .flex()
                                    .items_center()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(rgba(0x000000ff))
                                    .child(primary_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let mut close = false;
                                        if let Some(cp) = this.checkpoint_2026.as_mut() {
                                            if cp.traits.nitro_locked_selected && !cp.has_nitro {
                                                return;
                                            }
                                            if cp.slide == CheckpointSlide::Character {
                                                let _ = this
                                                    .requests
                                                    .send(complete_checkpoint_request(&cp.traits));
                                            }
                                            if let Some(next) = cp.slide.next() {
                                                cp.slide = next;
                                            } else {
                                                close = true;
                                            }
                                        }
                                        if close {
                                            this.close_checkpoint_2026();
                                        }
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_checkpoint_2026(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.close_checkpoint_2026();
                cx.notify();
                "checkpoint closed".to_owned()
            }
            "summary" => {
                self.open_checkpoint_2026(cx);
                if let Some(cp) = self.checkpoint_2026.as_mut() {
                    cp.slide = CheckpointSlide::Summary;
                }
                cx.notify();
                "checkpoint opened (summary)".to_owned()
            }
            "squad" => {
                self.open_checkpoint_2026(cx);
                if let Some(cp) = self.checkpoint_2026.as_mut() {
                    cp.slide = CheckpointSlide::Squad;
                }
                cx.notify();
                "checkpoint opened (squad)".to_owned()
            }
            _ => {
                self.open_checkpoint_2026(cx);
                "checkpoint opened (home)".to_owned()
            }
        }
    }
}

fn summary_stat_tile(label: &str, value: String) -> AnyElement {
    div()
        .w(px(128.))
        .p(px(14.))
        .rounded(px(10.))
        .bg(rgba(0x00000066))
        .border_1()
        .border_color(rgba(CYAN_DARK))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(24.))
                .font_weight(FontWeight(800.))
                .text_color(color("--white"))
                .child(value),
        )
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight(600.))
                .text_color(rgba(CYAN_PRIMARY))
                .child(label.to_uppercase()),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_925433_slides_intl_and_requests() {
        for key in [
            "0ZQw/X", "13/7kX", "7hB4kg", "I0v0Qv", "I3ltXO", "K3lovD", "OIgYlQ", "PDTjLN",
            "PoWNfe", "R3BPH+", "RDE0Sc", "URyqtP", "Ve9Ge6", "YqAjXy", "cpT0Cq", "fjcCk5",
            "i4jeWR", "kwmYkt", "oeb1vg", "sMOuuS", "w4m945",
        ] {
            assert!(dc_intl::has_key(key), "missing 925433 key {key}");
        }
        assert_eq!(CheckpointSlide::Home.next(), Some(CheckpointSlide::Voice));
        assert_eq!(CheckpointSlide::Home.prev(), None);
        assert_eq!(CheckpointSlide::Share.next(), None);
        assert!(matches!(
            fetch_checkpoint_request(),
            Request::Rest { route, .. } if route == "/checkpoint"
        ));
        assert!(matches!(
            complete_checkpoint_request(&CheckpointTraits::default()),
            Request::Rest { route, .. } if route == "/checkpoint/complete"
        ));
        assert!(matches!(
            reset_checkpoint_request(),
            Request::Rest { route, .. } if route == "/checkpoint/reset"
        ));
    }
}
