//! The detected-game row above voice and clips (483364 `ng`, 821095/450173/369409 styles).
use super::{Root, color, icon};
use crate::parity::Parity;
use dc_media::screen::SourceId;
use dc_state::running_games::Game;
use gpui::{AnyElement, Context, FontWeight, div, img, prelude::*, px, svg};

impl Root {
    fn panel_running_game(&self) -> Option<Game> {
        // A whole-display stream uses the stream-metadata card instead of the game (483364 `eh`).
        if self
            .own_started()
            .is_some_and(|stream| matches!(stream.info.id, SourceId::Display(_)))
        {
            return None;
        }
        #[cfg(all(feature = "capture", windows))]
        if self.clips.engine.test_game {
            return self.clips.engine.game.as_ref().map(|game| Game {
                id: game.application_id.clone(),
                name: Some(game.name.clone()),
                pid: Some(game.pid),
                ..Game::default()
            });
        }
        self.detect.store.visible_game().cloned()
    }

    pub(super) fn running_game_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let game = self.panel_running_game()?;
        let picture = self.overlay_game_icon(game.id.as_deref());
        if let Some(id) = game.id {
            let root = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = root.update(cx, |root, _| root.overlay_request_game_icon(&id));
            });
        }
        let picture = match picture {
            Some(picture) => img(picture)
                .size(px(32.))
                .rounded(px(8.))
                .into_any_element(),
            None => {
                icon("inventory_unknown_game", 32., "--interactive-text-default").into_any_element()
            }
        };
        let keys = self.clips.panel_keys();
        let hint = (self.clips_enabled()
            && !keys.is_empty()
            && self.pref_on("activity_privacy_setting", true)
            && self.stores.status != "invisible")
            .then(|| {
                let runs = dc_intl::format_runs("ESZwQX", &[("keybind", keys.join("+").into())]);
                let parts = runs.into_iter().map(|(text, tag)| {
                    if tag.as_deref() == Some("keybindHook") {
                        div()
                            .flex()
                            .gap(px(3.))
                            .children(keys.iter().cloned().map(keycap))
                            .into_any_element()
                    } else {
                        div().child(text.trim().to_owned()).into_any_element()
                    }
                });
                div()
                    .parity("running-game-clip-hint")
                    .mt(px(2.))
                    .h(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-subtle"))
                    .children(parts)
            });
        Some(
            div()
                .id("running-game-panel")
                .parity("running-game-panel")
                .w_full()
                .flex_none()
                .p(px(12.))
                .rounded_t(px(8.))
                .bg(color("--background-surface-high"))
                .border_b_1()
                .border_color(color("--border-muted"))
                .flex()
                .items_center()
                .child(div().size(px(32.)).flex_none().child(picture))
                .child(
                    div()
                        .ml(px(8.))
                        .min_w(px(0.))
                        .flex_1()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(16.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(game.name.unwrap_or_else(|| dc_intl::tr("Unknown").into())),
                        )
                        .children(hint),
                )
                .into_any_element(),
        )
    }
}

fn keycap(key: String) -> AnyElement {
    let glyph = match key.as_str() {
        "UP" => Some(std::f32::consts::PI),
        "DOWN" => Some(0.),
        "LEFT" => Some(std::f32::consts::FRAC_PI_2),
        "RIGHT" => Some(-std::f32::consts::FRAC_PI_2),
        _ => None,
    };
    let content = glyph.map_or_else(
        || key.into_any_element(),
        |angle| {
            svg()
                .path("icons/key_arrow.svg")
                .size(px(8.))
                .text_color(color("--interactive-text-active"))
                .with_transformation(gpui::Transformation::rotate(gpui::radians(angle)))
                .into_any_element()
        },
    );
    div()
        .flex_none()
        .h(px(16.))
        .min_w(px(16.))
        .px(px(4.))
        .pt(px(1.))
        .pb(px(2.))
        .rounded(px(3.))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-mod-muted"))
        .text_color(color("--interactive-text-active"))
        .text_size(px(10.))
        .line_height(px(10.))
        .font_weight(FontWeight(600.))
        .flex()
        .items_center()
        .justify_center()
        .child(content)
        .into_any_element()
}
