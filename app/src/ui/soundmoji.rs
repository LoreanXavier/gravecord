//! Soundmoji popout, Guild Info card, hover preview, and Volume slider (module 456086), and the soundmoji of a
//! message's text: `<sound:guild:id>` (the soundboard rule of module 605275, parsed by 113051) drawn as the inline
//! chip of module 632760 that plays the sound when clicked.
//!
//! GAP: a message of soundmoji alone (`jumboable`, module 354777 counts them with the emoji) draws the soundboard
//! button of module 533491 officially; here it stays the inline chip. The chip's hover border
//! (`hsla(0,0%,100%,.5)`), its pressed background and its box shadows are not drawn (the inline layout has no
//! per-piece border hover or shadow), and its line does not grow for the chip's 2px margins. The tooltip is the
//! sound's name over "Click to hear sound" (`D6eYmf`), without the BETA banner and soundboard icon of 456086 `H`.
//! The soundmoji volume is the setting's default (100, `soundmojiVolume` of module 253932): the app does not read
//! that user setting yet.

use super::{Root, color, icon};
use crate::inline::BoxStyle;
use dc_state::message_kinds::MessageSound;
use dc_state::{Id, Message, MessageState};
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, SharedString, deferred, div, prelude::*,
    px, rgba,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// The experiment soundmoji are drawn behind (module 864145); off by default, so a soundmoji is its raw token.
pub(super) const RENDERING_EXPERIMENT: &str = "2026-03-soundmoji-rendering";
/// The redraw a finished or started play asks for (not a gateway event).
pub(super) const PLAY_START: &str = "SOUNDMOJI_PLAY_START";
pub(super) const PLAY_END: &str = "SOUNDMOJI_PLAY_END";

type DecodedSounds = Mutex<HashMap<String, Arc<dc_voice::sound::Decoded>>>;

/// A sound a soundmoji names, as module 807348 `N0` makes the record.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Shown {
    pub id: String,
    pub guild: String,
    pub name: String,
    pub emoji_id: Option<String>,
    pub emoji_name: Option<String>,
    pub volume: f64,
}

impl From<&MessageSound> for Shown {
    fn from(sound: &MessageSound) -> Self {
        Self {
            id: sound.sound_id.clone(),
            guild: sound.guild_id.clone(),
            name: sound.name.clone(),
            emoji_id: sound.emoji_id.clone(),
            emoji_name: sound.emoji_name.clone(),
            volume: sound.volume,
        }
    }
}

impl From<&super::soundboard::Sound> for Shown {
    fn from(sound: &super::soundboard::Sound) -> Self {
        Self {
            id: sound.id.clone(),
            guild: sound.guild.clone(),
            name: sound.name.clone(),
            emoji_id: sound.emoji_id.clone(),
            emoji_name: sound.emoji_name.clone(),
            volume: sound.volume,
        }
    }
}

impl Shown {
    /// The emoji drawn in the chip, by the key the message's images use.
    pub(super) fn emoji_key(&self) -> Option<String> {
        match (&self.emoji_id, &self.emoji_name) {
            (Some(id), _) => Some(format!("custom:{id}")),
            (None, Some(name)) => Some(format!("emoji:{name}")),
            _ => None,
        }
    }
}

/// The message whose text is being drawn: what module 113051's parse state holds (`channelId`, `messageId`, the
/// message's `soundboardSounds`) and whether it was sent.
#[derive(Clone, Debug, Default)]
pub(super) struct Scope {
    pub channel: Id,
    pub sent: bool,
    pub sounds: Vec<MessageSound>,
}

thread_local! {
    static SCOPE: RefCell<Option<Scope>> = const { RefCell::new(None) };
    static PLAYING: RefCell<Option<(String, dc_voice::sound::Handle)>> = const { RefCell::new(None) };
}

/// The scope of one message's text; the previous one comes back when it is dropped.
pub(super) struct Entered(Option<Scope>);

impl Drop for Entered {
    fn drop(&mut self) {
        let previous = self.0.take();
        SCOPE.with(|scope| *scope.borrow_mut() = previous);
    }
}

/// Soundmoji drawn while the returned guard lives resolve against this message (a forward's copy carries its
/// snapshot's sounds, module 470020). Outside any message (a reply preview, an embed) a soundmoji stays its token.
pub(super) fn enter(message: &Message) -> Entered {
    let next = Scope {
        channel: message.channel_id,
        sent: message.state == MessageState::Sent,
        sounds: message.extra.soundboard_sounds.clone(),
    };
    Entered(SCOPE.with(|scope| scope.borrow_mut().replace(next)))
}

fn scope() -> Option<Scope> {
    SCOPE.with(|scope| scope.borrow().clone())
}

/// `f` of module 113051 with 470020 `A`: a sent message draws only the sounds it carries; one not sent yet (or a
/// text outside a message) may draw the soundboard store's sound when module 69945 allows it. `None` keeps the raw
/// token (also while the experiment is off).
pub(super) fn resolve(
    enabled: bool,
    scope: Option<&Scope>,
    sound: &str,
    store: Option<Shown>,
    can_render: bool,
) -> Option<Shown> {
    if !enabled {
        return None;
    }
    match scope {
        Some(scope) => {
            let carried = scope
                .sounds
                .iter()
                .find(|carried| carried.sound_id == sound)
                .map(Shown::from);
            if carried.is_none() && can_render && !scope.sent {
                return store;
            }
            carried
        }
        None => store.filter(|_| can_render),
    }
}

/// Module 69945: a default sound, one of this channel's server or of the text's server may be drawn; another
/// server's sound needs "use soundboard everywhere" (Nitro) and, in a server channel, USE_EXTERNAL_SOUNDS.
pub(super) fn can_render(
    sound_guild: Option<&str>,
    channel_guild: Option<Id>,
    everywhere: bool,
    external: bool,
) -> bool {
    let own = |guild: &str| {
        guild == super::soundboard::DISCORD
            || channel_guild.is_some_and(|channel| guild == channel.to_string())
    };
    sound_guild.is_some_and(own) || (everywhere && (channel_guild.is_none() || external))
}

/// The decoded sounds, by id.
fn decoded() -> &'static DecodedSounds {
    static DECODED: OnceLock<DecodedSounds> = OnceLock::new();
    DECODED.get_or_init(Mutex::default)
}

/// Whether the chip of this sound plays now (`isPlaying` of module 584014).
pub(super) fn playing(sound: &str) -> bool {
    PLAYING.with(|playing| {
        playing.borrow().as_ref().is_some_and(|(id, handle)| {
            id == sound && !handle.is_finished() && !handle.is_stopped()
        })
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SoundmojiButton {
    JoinGuild,
    GetNitro,
    None,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct SoundmojiContext {
    pub sound_name: String,
    pub sound_emoji: String,
    pub is_default_sound: bool,
    pub can_send_soundmojis: bool,
    pub has_soundmoji_permissions: bool,
    pub is_in_guild: bool,
    pub is_guild_discoverable: bool,
    pub is_sound_from_different_guild: bool,
    pub source_guild_name: Option<String>,
    pub volume_pct: u8,
    pub show_volume: bool,
}

/// Official `buttonType` & exhaustive `description` matrix (`B` of module 456086).
pub(crate) fn soundmoji_info(ctx: &SoundmojiContext) -> (SoundmojiButton, &'static str) {
    let button = if ctx.is_default_sound || !ctx.can_send_soundmojis {
        SoundmojiButton::None
    } else if ctx.has_soundmoji_permissions {
        if ctx.is_in_guild || !ctx.is_guild_discoverable {
            SoundmojiButton::None
        } else {
            SoundmojiButton::JoinGuild
        }
    } else {
        SoundmojiButton::GetNitro
    };

    let desc_key = if !ctx.can_send_soundmojis {
        "x2kyyJ"
    } else if ctx.is_default_sound {
        "AabHep"
    } else if !ctx.is_in_guild && !ctx.is_guild_discoverable {
        "MRYt06"
    } else if ctx.has_soundmoji_permissions {
        if ctx.is_in_guild && !ctx.is_sound_from_different_guild {
            "p17MQJ"
        } else if ctx.is_in_guild {
            "Lkbm5s"
        } else {
            "GTJmaS"
        }
    } else if ctx.is_in_guild && !ctx.is_sound_from_different_guild {
        "sj/imS"
    } else if ctx.is_in_guild {
        "3Ru2/x"
    } else {
        "qRkWhZ"
    };

    (button, dc_intl::t(desc_key))
}

impl Root {
    /// The sound a soundmoji of the text being drawn stands for, or `None` for its raw token.
    pub(super) fn soundmoji_sound(&self, sound: u64) -> Option<Shown> {
        let sound = sound.to_string();
        let enabled = self.stores.experiments.variant(RENDERING_EXPERIMENT) == Some(1);
        if !enabled {
            return None;
        }
        let scope = scope()?;
        let store = self
            .soundboard
            .defaults
            .iter()
            .chain(self.soundboard.guilds.values().flatten())
            .find(|known| known.id == sound)
            .map(Shown::from);
        let channel_guild = self.guild_of_channel(scope.channel);
        let me = self.stores.me();
        let external = match (channel_guild.and_then(|guild| self.stores.guild(guild)), me) {
            (Some(guild), Some(me)) => guild
                .channels
                .iter()
                .find(|channel| channel.id == scope.channel)
                .is_some_and(|channel| {
                    dc_state::permissions::can_in_channel(
                        guild,
                        channel,
                        me,
                        dc_state::permissions::USE_EXTERNAL_SOUNDS,
                    )
                }),
            _ => false,
        };
        let everywhere = dc_state::premium::PremiumType::from_wire(self.premium_wire())
            .has(dc_state::premium::Feature::SoundboardEverywhere);
        let allowed = can_render(
            store.as_ref().map(|known| known.guild.as_str()),
            channel_guild,
            everywhere,
            external,
        );
        resolve(enabled, Some(&scope), &sound, store, allowed)
    }

    /// The chip of module 632760 `_` (`inlineContainer_t3sUMN inlineButton_t3sUMN`): `--card-background-default` in a
    /// 1px `--border-strong` border (`--control-connected-border-default` while it plays) with `--radius-sm` corners,
    /// `0 --space-4` padding and a 1.5em line, the sound's emoji (1em, `soundmojiEmoji_t3sUMN`) before its name.
    pub(super) fn soundmoji_box(&self, sound: &Shown, size: f32, line: f32) -> BoxStyle {
        let inner = (size * 1.5).round();
        let height = inner + 2.;
        let top = ((line - height) / 2.).floor();
        let picture = sound
            .emoji_key()
            .and_then(|key| self.images.get(&key).cloned())
            .map(|image| (image, px(size)));
        BoxStyle {
            icon: None,
            picture,
            padding_x: px(4.),
            height: px(height),
            top: px(top),
            background: Hsla::from(color("--card-background-default")),
            border: Hsla::from(color(if playing(&sound.id) {
                "--control-connected-border-default"
            } else {
                "--border-strong"
            })),
            radius: px(8.),
            text_line_height: px(inner),
            text_top: px(top + 1.),
        }
    }

    /// `v` of module 632760: the sound plays here on the default speaker (584014's audio element), at
    /// `min(volume * M(soundmojiVolume) / 100 * min(outputVolume / 100, 1), 1)` (module 904054); a new play stops the
    /// one before it. Nothing is sent.
    pub(super) fn play_soundmoji(&mut self, sound: &Shown) {
        if sound.id.is_empty() || !sound.id.bytes().all(|byte| byte.is_ascii_digit()) {
            return;
        }
        let output_gain = dc_voice::prefs().output_gain();
        let official = super::soundboard::playback_volume(sound.volume, 100., output_gain);
        let factor = super::soundboard::engine_factor(official, output_gain);
        let handle = dc_voice::sound::Handle::new();
        if let Some((_, before)) = PLAYING.with(|playing| {
            playing
                .borrow_mut()
                .replace((sound.id.clone(), handle.clone()))
        }) {
            before.stop();
        }
        let (updates, id) = (self.updates.clone(), sound.id.clone());
        let spawned = std::thread::Builder::new()
            .name("gravecord-soundmoji".into())
            .spawn(move || {
                let cached = decoded()
                    .lock()
                    .ok()
                    .and_then(|known| known.get(&id).cloned());
                let sound = match cached {
                    Some(sound) => sound,
                    None => {
                        let url = format!("https://cdn.discordapp.com/soundboard-sounds/{id}");
                        let Ok(bytes) = crate::sounds::fetch_url(&url) else {
                            tracing::debug!("soundmoji file unavailable");
                            return;
                        };
                        let Ok(sound) = dc_voice::sound::decode_any(bytes) else {
                            tracing::debug!("soundmoji file not playable");
                            return;
                        };
                        let sound = Arc::new(sound);
                        if let Ok(mut known) = decoded().lock() {
                            known.insert(id.clone(), sound.clone());
                        }
                        sound
                    }
                };
                if handle.is_stopped() {
                    return;
                }
                let event = |name: &str| crate::net::Update::Dispatch {
                    name: name.to_owned(),
                    data: serde_json::json!({"sound_id": id}),
                };
                let _ = updates.send(event(PLAY_START));
                dc_voice::sound::play(
                    sound.clone(),
                    factor,
                    dc_voice::sound::Output::Default,
                    &handle,
                );
                let (started, length) = (std::time::Instant::now(), sound.duration());
                while !handle.is_stopped() && !handle.is_finished() && started.elapsed() < length {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                let _ = updates.send(event(PLAY_END));
            });
        if spawned.is_err() {
            tracing::debug!("could not start the soundmoji player");
        }
    }

    pub(crate) fn close_soundmoji_popout(&mut self) -> bool {
        self.soundmoji_popout.take().is_some()
    }

    pub(in crate::ui) fn soundmoji_popout_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let ctx = self.soundmoji_popout.as_ref()?;
        let (btn_kind, description) = soundmoji_info(ctx);
        let show_nitro_banner = !ctx.has_soundmoji_permissions
            && !ctx.is_default_sound
            && ctx.is_sound_from_different_guild
            && ctx.can_send_soundmojis;

        let banner: AnyElement = if show_nitro_banner {
            div()
                .px(px(14.))
                .py(px(8.))
                .bg(color("--control-primary-background-default"))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(icon("nitro_wheel", 14., "--white"))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("BMw+7I")),
                        ),
                )
                .child(
                    div()
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .bg(rgba(0x00000044))
                        .text_size(px(10.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--white"))
                        .child("BETA"),
                )
                .into_any_element()
        } else {
            div()
                .px(px(14.))
                .pt(px(12.))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .bg(color("--control-primary-background-default"))
                        .text_size(px(10.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--white"))
                        .child("BETA"),
                )
                .child(
                    div()
                        .id("soundmoji-close")
                        .size(px(24.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("modal_close", 14., "--interactive-icon-default"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_soundmoji_popout();
                            cx.notify();
                        })),
                )
                .into_any_element()
        };

        let cta = match btn_kind {
            SoundmojiButton::GetNitro => Some(
                div()
                    .id("soundmoji-get-nitro")
                    .h(px(34.))
                    .w_full()
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .text_size(px(13.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--white"))
                    .child(icon("nitro_wheel", 14., "--white"))
                    .child(dc_intl::t("pj0XBN"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_soundmoji_popout();
                        this.open_nitro(cx);
                    })),
            ),
            SoundmojiButton::JoinGuild => Some(
                div()
                    .id("soundmoji-join-guild")
                    .h(px(34.))
                    .w_full()
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--white"))
                    .child(dc_intl::t("riu2R5"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_soundmoji_popout();
                        cx.notify();
                    })),
            ),
            SoundmojiButton::None => None,
        };

        let vol = ctx.volume_pct.min(100);
        let volume_panel = ctx.show_volume.then(|| {
            div()
                .p(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-lower"))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("2JbvKw")),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(color("--text-muted"))
                                .child(format!("{vol}%")),
                        ),
                )
                .child(div().flex().items_center().gap(px(6.)).children(
                    [0u8, 25, 50, 75, 100].into_iter().map(|step| {
                        let active = vol == step;
                        div()
                            .id(SharedString::from(format!("soundmoji-vol-{step}")))
                            .flex_1()
                            .h(px(24.))
                            .rounded(px(6.))
                            .bg(if active {
                                color("--control-primary-background-default")
                            } else {
                                color("--control-secondary-background-default")
                            })
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(11.))
                            .text_color(if active {
                                color("--white")
                            } else {
                                color("--text-default")
                            })
                            .child(format!("{step}%"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(s) = this.soundmoji_popout.as_mut() {
                                    s.volume_pct = step;
                                }
                                cx.notify();
                            }))
                    }),
                ))
        });

        let card = div()
            .id("soundmoji-popout-card")
            .occlude()
            .w(px(340.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.3),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(banner)
            .child(
                div()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .size(px(40.))
                                            .rounded(px(10.))
                                            .bg(color("--background-surface-higher"))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(20.))
                                            .child(ctx.sound_emoji.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .text_size(px(15.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child(ctx.sound_name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(color("--text-muted"))
                                                    .child(dc_intl::t("D6eYmf")),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(
                                        self.with_tip(
                                            "soundmoji-vol-tip",
                                            super::tooltip::Tip::text(
                                                dc_intl::t("19lt24"),
                                                super::tooltip::Side::Top,
                                            ),
                                            div()
                                                .id("soundmoji-vol-toggle")
                                                .size(px(28.))
                                                .rounded(px(6.))
                                                .bg(color("--control-secondary-background-default"))
                                                .hover(|s| {
                                                    s.bg(color(
                                                        "--control-secondary-background-hover",
                                                    ))
                                                })
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(icon(
                                                    "sb_play",
                                                    14.,
                                                    "--interactive-icon-default",
                                                ))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(s) = this.soundmoji_popout.as_mut()
                                                    {
                                                        s.show_volume = !s.show_volume;
                                                    }
                                                    cx.notify();
                                                })),
                                            cx,
                                        ),
                                    )
                                    .child(
                                        self.with_tip(
                                            "soundmoji-info-tip",
                                            super::tooltip::Tip::text(
                                                dc_intl::t("KVbJU/"),
                                                super::tooltip::Side::Top,
                                            ),
                                            div()
                                                .id("soundmoji-info-btn")
                                                .size(px(28.))
                                                .rounded(px(6.))
                                                .bg(color("--control-secondary-background-default"))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(icon(
                                                    "circle_info",
                                                    14.,
                                                    "--interactive-icon-default",
                                                )),
                                            cx,
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(description),
                    )
                    .children(volume_panel)
                    .when_some(ctx.source_guild_name.clone(), |col, gname| {
                        col.child(
                            div()
                                .p(px(10.))
                                .rounded(px(8.))
                                .bg(color("--background-base-lower"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-muted"))
                                        .child(if ctx.is_in_guild {
                                            dc_intl::t("tGDabk")
                                        } else {
                                            dc_intl::t("rnOmOa")
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(gname),
                                ),
                        )
                    })
                    .children(cta),
            );

        Some(
            deferred(
                div()
                    .id("soundmoji-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_soundmoji_popout();
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_soundmoji(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        self.soundmoji_popout = Some(SoundmojiContext {
            sound_name: "Airhorn".to_owned(),
            sound_emoji: "📯".to_owned(),
            is_default_sound: arg == "default",
            can_send_soundmojis: arg != "beta",
            has_soundmoji_permissions: arg == "join",
            is_in_guild: arg == "same",
            is_guild_discoverable: arg != "private",
            is_sound_from_different_guild: arg != "same",
            source_guild_name: if arg == "default" {
                None
            } else {
                Some("gravecord fixture".to_owned())
            },
            volume_pct: 75,
            show_volume: true,
        });
        cx.notify();
        format!("soundmoji: {arg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carried(id: &str, name: &str) -> MessageSound {
        MessageSound {
            sound_id: id.to_owned(),
            guild_id: "0".to_owned(),
            name: name.to_owned(),
            volume: 1.,
            emoji_id: None,
            emoji_name: Some("\u{1f986}".to_owned()),
            available: true,
        }
    }

    fn stored(id: &str, guild: &str) -> Shown {
        Shown {
            id: id.to_owned(),
            guild: guild.to_owned(),
            name: "stored".to_owned(),
            emoji_id: None,
            emoji_name: None,
            volume: 1.,
        }
    }

    /// Modules 113051 and 470020: off, the token stays; a sent message draws only what it carries; one being sent
    /// may fall back on the store; no message at all needs module 69945's leave.
    #[test]
    fn a_soundmoji_resolves_from_its_message_first() {
        let sent = Scope {
            channel: 1,
            sent: true,
            sounds: vec![carried("5", "quack")],
        };
        assert_eq!(resolve(false, Some(&sent), "5", None, true), None);
        let shown = resolve(true, Some(&sent), "5", Some(stored("5", "0")), true).unwrap();
        assert_eq!(shown.name, "quack");
        assert_eq!(shown.emoji_key().as_deref(), Some("emoji:\u{1f986}"));
        assert_eq!(
            resolve(true, Some(&sent), "6", Some(stored("6", "0")), true),
            None
        );
        let sending = Scope {
            sent: false,
            ..sent.clone()
        };
        assert_eq!(
            resolve(true, Some(&sending), "6", Some(stored("6", "0")), true).map(|s| s.name),
            Some("stored".to_owned())
        );
        assert_eq!(
            resolve(true, Some(&sending), "6", Some(stored("6", "9")), false),
            None
        );
        assert!(resolve(true, None, "6", Some(stored("6", "0")), true).is_some());
        assert!(resolve(true, None, "6", Some(stored("6", "0")), false).is_none());
    }

    /// Module 69945.
    #[test]
    fn another_servers_sound_needs_nitro_and_external_sounds() {
        assert!(can_render(Some("0"), Some(4), false, false));
        assert!(can_render(Some("4"), Some(4), false, false));
        assert!(!can_render(Some("7"), Some(4), false, true));
        assert!(!can_render(Some("7"), Some(4), true, false));
        assert!(can_render(Some("7"), Some(4), true, true));
        assert!(can_render(Some("7"), None, true, false));
        assert!(can_render(None, None, true, false));
        assert!(!can_render(None, None, false, false));
    }

    #[test]
    fn soundmoji_info_covers_all_9_branches_of_456086() {
        let base = SoundmojiContext {
            sound_name: "Airhorn".to_owned(),
            sound_emoji: "📯".to_owned(),
            is_default_sound: false,
            can_send_soundmojis: true,
            has_soundmoji_permissions: true,
            is_in_guild: true,
            is_guild_discoverable: true,
            is_sound_from_different_guild: false,
            source_guild_name: Some("Guild".to_owned()),
            volume_pct: 80,
            show_volume: false,
        };
        assert_eq!(soundmoji_info(&base).0, SoundmojiButton::None);
        assert!(soundmoji_info(&base).1.contains("this server"));

        let mut c = base.clone();
        c.can_send_soundmojis = false;
        assert!(soundmoji_info(&c).1.contains("Beta"));

        let mut c = base.clone();
        c.is_default_sound = true;
        assert!(soundmoji_info(&c).1.contains("default sound"));

        let mut c = base.clone();
        c.is_in_guild = false;
        c.is_guild_discoverable = false;
        assert!(soundmoji_info(&c).1.contains("invite-only"));

        let mut c = base.clone();
        c.is_sound_from_different_guild = true;
        assert!(soundmoji_info(&c).1.contains("one of your servers"));

        let mut c = base.clone();
        c.is_in_guild = false;
        assert_eq!(soundmoji_info(&c).0, SoundmojiButton::JoinGuild);
        assert!(soundmoji_info(&c).1.contains("Join the server"));

        let mut c = base.clone();
        c.has_soundmoji_permissions = false;
        assert_eq!(soundmoji_info(&c).0, SoundmojiButton::GetNitro);
        assert!(soundmoji_info(&c).1.contains("Get Nitro"));

        let mut c = base.clone();
        c.has_soundmoji_permissions = false;
        c.is_sound_from_different_guild = true;
        assert!(soundmoji_info(&c).1.contains("Get Nitro"));

        let mut c = base;
        c.has_soundmoji_permissions = false;
        c.is_in_guild = false;
        assert!(soundmoji_info(&c).1.contains("Get Nitro and join"));
    }
}
