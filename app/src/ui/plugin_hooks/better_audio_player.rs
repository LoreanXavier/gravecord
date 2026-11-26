//! BetterAudioPlayer (Equicord `src/equicordplugins/betterAudioPlayer.desktop`, GPL-3.0-or-later): the oscilloscope and
//! spectrograph over the audio player (module 153440 type AUDIO, the attachment card and the audio embed), drawn from
//! the player's decoded samples (`plugins/better_audio_player.rs`). The original's canvas fills the player's wrapper
//! (`position: absolute`, 100% by 100%, no pointer events) and draws every animation frame while the player plays;
//! when it stops, the last frame stays. A player that never played draws nothing. Not part of the pinned client.

use super::super::Root;
use super::super::voice_message::Load;
use crate::plugins::State;
use crate::plugins::better_audio_player::{Analyser, Frame, bars, parse_color, scope};
use dc_voice::voice_player;
use gpui::{
    AnyElement, IntoElement, PathBuilder, Styled, canvas, fill, linear_color_stop, linear_gradient,
    point, px, rgb,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

const ID: &str = "BetterAudioPlayer";
const OSCILLOSCOPE_DEFAULT: [u8; 3] = [255, 255, 255];
const SPECTROGRAPH_DEFAULT: [u8; 3] = [33, 150, 243];
/// Players whose last frame is kept (the original's canvases live as long as their players).
const KEPT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Settings {
    oscilloscope: bool,
    spectrograph: bool,
    oscilloscope_solid: bool,
    oscilloscope_color: [u8; 3],
    spectrograph_solid: bool,
    spectrograph_color: [u8; 3],
}

impl Settings {
    fn read(state: &State) -> Option<Self> {
        if !state.is_on(ID) {
            return None;
        }
        let flag = |key| state.value(ID, key) == Value::Bool(true);
        let text = |key| state.value(ID, key).as_str().unwrap_or_default().to_owned();
        Some(Self {
            oscilloscope: flag("oscilloscope"),
            spectrograph: flag("spectrograph"),
            oscilloscope_solid: flag("oscilloscopeSolidColor"),
            oscilloscope_color: parse_color(&text("oscilloscopeColor"), OSCILLOSCOPE_DEFAULT),
            spectrograph_solid: flag("spectrographSolidColor"),
            spectrograph_color: parse_color(&text("spectrographColor"), SPECTROGRAPH_DEFAULT),
        })
    }
}

/// A player's analyser and the frame it last read.
struct Player {
    key: String,
    analyser: Analyser,
    frame: Rc<Frame>,
}

thread_local! {
    /// The settings as last read, with the plugins' revision they were read at.
    static SETTINGS: RefCell<Option<(u64, Option<Settings>)>> = const { RefCell::new(None) };
    static PLAYERS: RefCell<VecDeque<Player>> = const { RefCell::new(VecDeque::new()) };
}

fn settings() -> Option<Settings> {
    if crate::plugins::bypassed() || !crate::plugins::on(ID) {
        return None;
    }
    let revision = crate::plugins::revision();
    SETTINGS.with(|cell| {
        let mut cell = cell.borrow_mut();
        match *cell {
            Some((at, settings)) if at == revision => settings,
            _ => {
                let settings = crate::plugins::with(Settings::read);
                *cell = Some((revision, settings));
                settings
            }
        }
    })
}

/// The frame to draw for the player `key`: read anew while it plays, the last one while it does not.
fn frame_of(key: &str, samples: Option<&[f32]>) -> Option<Rc<Frame>> {
    let status = voice_player::engine().status();
    let playing = status.playing && status.key.as_deref() == Some(key);
    PLAYERS.with(|players| {
        let mut players = players.borrow_mut();
        let found = players.iter().position(|player| player.key == key);
        let (Some(samples), true) = (samples, playing) else {
            return found.map(|at| players[at].frame.clone());
        };
        let at = match found {
            Some(at) => at,
            None => {
                if players.len() >= KEPT {
                    players.pop_front();
                }
                players.push_back(Player {
                    key: key.to_owned(),
                    analyser: Analyser::new(),
                    frame: Rc::new(Frame::default()),
                });
                players.len() - 1
            }
        };
        let player = &mut players[at];
        let sample = (status.position.max(0.) * 48_000.) as usize;
        // The last frame's drawing has let go of it by now, so this writes in place.
        player
            .analyser
            .read(samples, sample, Rc::make_mut(&mut player.frame));
        Some(player.frame.clone())
    })
}

fn tint(color: [u8; 3]) -> gpui::Rgba {
    rgb(u32::from(color[0]) << 16 | u32::from(color[1]) << 8 | u32::from(color[2]))
}

impl Root {
    /// The visualiser canvas of the audio player `key`, laid over its wrapper; nothing when the plugin is off, both
    /// visualisers are off, or the player has not played yet.
    pub(in crate::ui) fn better_audio_player(&self, key: &str) -> Option<AnyElement> {
        let settings = settings()?;
        if !settings.oscilloscope && !settings.spectrograph {
            return None;
        }
        let clip = match self.voice_hub.borrow().load(key) {
            Some(Load::Ready(clip)) => Some(clip),
            _ => None,
        };
        let frame = frame_of(key, clip.as_ref().map(|clip| clip.samples()))?;
        crate::plugins::note(ID);
        Some(
            canvas(
                |_, _, _| {},
                // The canvas draws inside its own box only: the scope's three-fold swing is cut at its edges.
                move |bounds, _, window, _| {
                    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                        let (width, height) =
                            (f32::from(bounds.size.width), f32::from(bounds.size.height));
                        let origin = bounds.origin;
                        if settings.oscilloscope {
                            let (points, color) = scope(
                                &frame.time,
                                width,
                                height,
                                settings.oscilloscope_color,
                                settings.oscilloscope_solid,
                            );
                            let mut path = PathBuilder::stroke(px(2.));
                            for (index, (x, y)) in points.into_iter().enumerate() {
                                let at = point(origin.x + px(x), origin.y + px(y));
                                if index == 0 {
                                    path.move_to(at);
                                } else {
                                    path.line_to(at);
                                }
                            }
                            if let Ok(path) = path.build() {
                                window.paint_path(path, tint(color));
                            }
                        }
                        if settings.spectrograph {
                            for bar in bars(
                                &frame.frequency,
                                width,
                                height,
                                settings.spectrograph_color,
                                settings.spectrograph_solid,
                            ) {
                                let area = gpui::Bounds::new(
                                    point(origin.x + px(bar.x), origin.y + px(bar.top)),
                                    gpui::size(px(bar.width), px(bar.height)),
                                );
                                let background = if bar.top_color == bar.bottom_color {
                                    tint(bar.top_color).into()
                                } else {
                                    linear_gradient(
                                        180.,
                                        linear_color_stop(tint(bar.top_color), 0.),
                                        linear_color_stop(tint(bar.bottom_color), 1.),
                                    )
                                };
                                window.paint_quad(fill(area, background));
                            }
                        }
                    });
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .into_any_element(),
        )
    }
}
