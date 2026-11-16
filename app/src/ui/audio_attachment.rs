//! The audio attachment card: a plain .mp3/.ogg/.wav/... file that is not a voice message.
//!
//! Sources, all in the pinned build: the card is `renderAudioComponent` `Nj` of module 548146 into `G` and the media
//! player `eh` of module 153440 (type AUDIO: metadata `er`, controls `el`, time `en`), with the stylesheets of
//! `wrapperAudio_FJ99sr` and `mediaBar*_s1cr3c`; the hover buttons are `P` of module 96782 (`hoverButtonGroup_n2rO26`
//! with `nonMediaMosaicItem_n2rO26`). Measured in `design/dom/audio-attachment/*.json`.
//!
//! The player keeps the official states: the metadata (the length) loads the first time the pointer is over the card
//! (`preload: "none"` then `"metadata"`), the time reads "-:-- / -:--" until then, the play button turns into pause
//! while playing and into the replay arrow when the end is reached, a click or drag on the bar seeks (a drag pauses
//! and resumes at its end), and the volume button toggles the mute with the vertical slider over it. The sound goes
//! through the same engine as the voice messages (the silent one in capture runs).

use super::tooltip::{Side, Tip};
use super::voice_message::{Hub, Load, fraction_of};
use super::{MONO, Root, color, icon};
use crate::parity::Parity;
use dc_state::{Attachment, Message};
use dc_tokens::Theme;
use dc_voice::voice_player;
use gpui::{
    AnyElement, Bounds, Context, DispatchPhase, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, SharedString, canvas, div, img, point,
    prelude::*, px, relative,
};

/// The prefix of the hub key of an audio attachment: what tells the loader to decode any file.
pub(super) const KEY: &str = "audio:";

/// The controls sit in a `theme-dark theme-midnight` wrapper: `--interactive-text-active` reads the midnight value
/// (`oklab(0.895351 ...)`, 220 220 223); the other tokens of the card are the same in both themes.
pub(super) fn control_tint() -> gpui::Rgba {
    gpui::rgba(Theme::Midnight.color("--interactive-text-active"))
}

/// `controlIcon_FJ99sr`: a 24 px glyph in the control tint.
pub(super) fn control_icon(name: &str) -> gpui::Svg {
    gpui::svg()
        .path(format!("icons/{name}.svg"))
        .size(px(24.))
        .flex_none()
        .text_color(control_tint())
}

/// `ei` in module 153440: `m:ss` of the whole seconds (`0|seconds`).
pub(super) fn clock(seconds: f64) -> String {
    let whole = if seconds.is_finite() && seconds > 0. {
        seconds as u64
    } else {
        0
    };
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// `en`: the elapsed and the whole length, "-:--" while unknown, the elapsed padded with zeros to the length's width.
pub(super) fn clock_pair(current: Option<f64>, duration: Option<f64>) -> (String, String) {
    let elapsed = current.map_or_else(|| "-:--".to_owned(), clock);
    let whole = duration.map_or_else(|| "-:--".to_owned(), clock);
    let padded = format!("{:0>width$}", elapsed, width = whole.chars().count());
    (padded, whole)
}

/// Module 153440 `handleBuffer`: the rail's buffered part is read from the audio element's `buffered` ranges on the
/// `progress` event, debounced 400 ms (trailing), and a range shorter than one second is skipped. The whole file is
/// fetched and decoded here when the pointer enters the card, so the range is the whole clip from the moment the load
/// completes (the one `progress` the element would fire for a small file), shown 400 ms later. A file the element
/// would only have buffered in part cannot be told apart without range requests.
pub(super) const BUFFER_DELAY_MS: f64 = 400.;
pub(super) const BUFFER_MIN_SECONDS: f64 = 1.;

/// What one frame shows of a card.
struct View {
    /// The length, once the metadata is in.
    duration: Option<f64>,
    current: f64,
    playing: bool,
    /// The end was reached: the button offers a replay.
    ended: bool,
    /// Something of the file was played or sought: the rail shows the buffered part (a paused card that never
    /// played has none).
    buffered: bool,
    slider_open: bool,
}

impl Hub {
    fn audio_view(&mut self, key: &str) -> View {
        let load = self.load(key);
        let duration = match &load {
            Some(Load::Ready(clip)) => Some(clip.seconds()),
            _ => None,
        };
        let span = duration.unwrap_or(0.);
        self.start_if_ready(key, span);
        let status = voice_player::engine().status();
        let me = status.key.as_deref() == Some(key);
        let now = super::motion::now();
        let slider_drag = self.volume_drag.as_deref() == Some(key);
        let wants = self.pills.get(key).is_some_and(|pill| pill.want_play);
        let pill = self.pill(key);
        if me && pill.drag.is_none() {
            pill.cached = status.position.min(span);
        }
        let playing = me && status.playing;
        let ended = duration.is_some_and(|whole| {
            pill.cached > 0.
                && clock(pill.cached) == clock(whole)
                && !playing
                && pill.drag.is_none()
        });
        let loading = matches!(load, Some(Load::Loading)) || wants && duration.is_none();
        if duration.is_some() && pill.ready_at.is_none() {
            pill.ready_at = Some(now);
        }
        // The rail's buffered part: the whole clip, 400 ms after the load completed (`handleBuffer`).
        let long_enough = duration.is_some_and(|whole| whole >= BUFFER_MIN_SECONDS);
        let buffered = long_enough && pill.ready_at.is_some_and(|at| now - at >= BUFFER_DELAY_MS);
        let filling = pill.ready_at.is_some_and(|at| now - at < BUFFER_DELAY_MS);
        if playing || loading || filling {
            super::motion::keep_animating();
        }
        View {
            duration,
            current: pill.cached,
            playing,
            ended,
            buffered,
            slider_open: slider_drag || pill.over > 0 || pill.hide_at.is_some_and(|at| now < at),
        }
    }
}

/// Deterministic hub key and GPUI element salt for an audio embed (`(message_id, embed_index, proxy_url)`).
pub(super) fn embed_audio_key(
    message_id: dc_state::Id,
    embed_index: usize,
    proxy_url: &str,
) -> (String, usize) {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in message_id
        .to_le_bytes()
        .into_iter()
        .chain((embed_index as u64).to_le_bytes())
        .chain(proxy_url.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let key = format!("{KEY}embed:{message_id}:{embed_index}:{hash:016x}");
    let salt = 0x4000_0000usize | ((hash as usize) & 0x3fff_ffff);
    (key, salt)
}

struct AudioBar<'a> {
    key: &'a str,
    salt: usize,
    span: f64,
    fraction: f32,
    loaded: bool,
    buffered: bool,
    over: bool,
    preview: f32,
    dragging: bool,
    playable: bool,
}

impl Root {
    /// `audioControls_FJ99sr`: the 32 px controls row shared by audio attachments and audio embeds (`eh` in module 153440).
    fn audio_controls(
        &self,
        key: &str,
        url: &str,
        salt: usize,
        playable: bool,
    ) -> (AnyElement, bool) {
        let hub = self.voice_hub.clone();
        if !playable {
            let status = voice_player::engine().status();
            let active = status.key.as_deref() == Some(key) && status.playing;
            let mut guard = hub.borrow_mut();
            let span = match guard.load(key) {
                Some(Load::Ready(clip)) => clip.seconds(),
                _ => 0.,
            };
            if active {
                guard.toggle(key, url, span);
            }
            let pill = guard.pill(key);
            pill.want_play = false;
            pill.over = 0;
            pill.bar_over = false;
            pill.hide_at = None;
        }
        let view = hub.borrow_mut().audio_view(key);
        let (volume, muted, bar_over, preview, dragging) = {
            let mut hub = hub.borrow_mut();
            let (volume, muted) = (hub.volume, hub.muted);
            let pill = hub.pill(key);
            (
                volume,
                muted,
                pill.bar_over,
                pill.preview,
                pill.drag.is_some(),
            )
        };
        let span = view.duration.unwrap_or(0.);
        let loaded = view.duration.is_some();
        let (elapsed, whole) = clock_pair(loaded.then_some(view.current), view.duration);
        let fraction = if span > 0. {
            (view.current / span).clamp(0., 1.) as f32
        } else {
            0.
        };

        // `controlIcon_FJ99sr`: 24 px in a 32 px box, `--interactive-text-active`, white when hovered.
        let play_icon = if view.playing {
            "clips_pause"
        } else if view.ended {
            "media_replay"
        } else {
            "clips_play"
        };
        let play = {
            let (hub, key, url) = (hub.clone(), key.to_owned(), url.to_owned());
            div()
                .id(("audio-play", salt))
                .flex_none()
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .text_color(control_tint())
                .when(playable, |this| {
                    this.cursor_pointer()
                        .hover(|style| style.text_color(color("--white")))
                        .on_click(move |_, window, _| {
                            hub.borrow_mut().toggle(&key, &url, span);
                            window.refresh();
                        })
                })
                .child(control_icon(play_icon))
        };

        // `durationTimeWrapper_FJ99sr`: 12 px mono, 7.2 px a character, a 2 px margin round the slash.
        let digits = |text: String| {
            let width = text.chars().count() as f32 * 7.2;
            div()
                .flex_none()
                .w(px(width))
                .h(px(12.))
                // The box is exactly the digits' width: at fractional zoom a hair more would wrap the last one.
                .whitespace_nowrap()
                .child(text)
                .into_any_element()
        };
        let time = div()
            .flex()
            .flex_none()
            .m(px(4.))
            .h(px(12.))
            .font_family(MONO)
            .text_size(px(12.))
            .line_height(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(color("--white"))
            .child(digits(elapsed))
            .child(
                div()
                    .flex_none()
                    .mx(px(2.))
                    .w(px(7.2))
                    .h(px(12.))
                    .child("/"),
            )
            .child(digits(whole));

        let bar = self.audio_bar(AudioBar {
            key,
            salt,
            span,
            fraction,
            loaded,
            buffered: view.buffered,
            over: bar_over,
            preview,
            dragging,
            playable,
        });

        let button = {
            let click_hub = hub.clone();
            let (hover_hub, hover_key) = (hub.clone(), key.to_owned());
            let level = if muted || volume <= 0. {
                "clips_voice_off"
            } else if volume < 0.5 {
                "vm_voice_low"
            } else {
                "clips_voice_on"
            };
            div()
                .id(("audio-volume", salt))
                .flex_none()
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .text_color(control_tint())
                .when(playable, |this| {
                    this.cursor_pointer()
                        .hover(|style| style.text_color(color("--white")))
                        .on_click(move |_, window, _| {
                            click_hub.borrow_mut().toggle_mute();
                            window.refresh();
                        })
                        .on_hover(move |hovered, window, _| {
                            let mut hub = hover_hub.borrow_mut();
                            let pill = hub.pill(&hover_key);
                            if *hovered {
                                pill.over = 1;
                                pill.hide_at = None;
                            } else {
                                pill.over = 0;
                                // The slider hides 150 ms after the pointer leaves (`_hoverTimeout`).
                                pill.hide_at = Some(super::motion::now() + 150.);
                            }
                            window.refresh();
                        })
                })
                .child(control_icon(level))
        };
        let mut volume_box = div().relative().flex_none().size(px(32.)).child(button);
        if playable && view.slider_open {
            let position = super::audio_menu::volume_to_slider(volume * 100.) / 100.;
            let position = if muted { 0. } else { position };
            // The slider box is 4 px above the controls row, 4 px in from the button's left edge plus its 4 px.
            volume_box = volume_box.child(self.voice_slider(key, salt, position, 36., 8.));
        }

        let controls = div()
            .flex()
            .items_center()
            .w_full()
            .h(px(32.))
            .mt(px(4.))
            .rounded(px(8.))
            .bg(color("--opacity-black-40"))
            .child(div().flex_none().mr(px(8.)).child(play))
            .child(time)
            .child(bar)
            .child(volume_box)
            .into_any_element();
        (controls, playable && view.slider_open)
    }

    /// `renderAudio` of module 265705 (`eh` in module 153440 with `fileName == null && fileSize == null`):
    /// `wrapperAudio_FJ99sr` with only the 32 px controls bar and no metadata row or hover buttons.
    pub(super) fn embed_audio_player(
        &self,
        message_id: dc_state::Id,
        embed_index: usize,
        proxy_url: &str,
        playable: bool,
    ) -> AnyElement {
        let (key, salt) = embed_audio_key(message_id, embed_index, proxy_url);
        let (controls, _) = self.audio_controls(&key, proxy_url, salt, playable);
        let over_hub = self.voice_hub.clone();
        let over_url = proxy_url.to_owned();
        div()
            .id(("embed-audio-card", salt))
            .parity("embed-audio")
            .relative()
            .flex()
            .flex_col()
            .w(px(432.))
            .max_w_full()
            .p(px(16.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-lower"))
            .when(playable, move |this| {
                this.on_hover(move |hovered, window, _| {
                    let mut hub = over_hub.borrow_mut();
                    if *hovered {
                        hub.request(&key, &over_url);
                    }
                    window.refresh();
                })
            })
            .child(controls)
            .into_any_element()
    }

    /// The card of one audio attachment of `message`.
    pub(super) fn audio_card(
        &self,
        message: &Message,
        attachment: &Attachment,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hub = self.voice_hub.clone();
        let key = format!("{KEY}{}:{}", message.id, attachment.id);
        let salt = (attachment.id % 1_000_000) as usize;
        let url = attachment.url.clone();
        let own = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| me.id == message.author.id);
        let removable = own && (message.attachments.len() > 1 || !message.content.is_empty());

        let group = SharedString::from(format!("audio-card-{salt}"));
        let download = attachment.clone();
        let name_click = attachment.clone();

        // `audioMetadata_FJ99sr`: the 24 by 32 file picture (40 high box), then the name and the size.
        let metadata = div()
            .flex()
            .child(
                // The `::before` picture: 24 px wide, shrinks with the row (the long name's picture is 15 by 20),
                // 24:32 whatever the width, top aligned in the 40 px box.
                div()
                    .flex_shrink(1.)
                    .w(px(24.))
                    .min_w(px(0.))
                    .h(px(40.))
                    .child(
                        div()
                            .w_full()
                            .aspect_ratio(24. / 32.)
                            .child(img("icons/file_audio.svg").size_full()),
                    ),
            )
            .child(
                div()
                    // `flex: 1 1 auto`: the name's own width counts, so a long one takes room from the picture.
                    .flex_grow(1.)
                    .flex_shrink(1.)
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .flex()
                    .flex_col()
                    .items_start()
                    .px(px(8.))
                    .child(
                        div()
                            .id(("audio-name", salt))
                            .text_size(px(16.))
                            .line_height(px(16.))
                            .text_color(color("--text-link"))
                            .cursor_pointer()
                            .hover(|style| style.underline())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                // The name is a plain anchor (`href` = the file): the link interceptor opens it.
                                this.follow_link(&name_click.url, &name_click.filename, cx);
                            }))
                            // The anchor under a right press gives the message menu its link rows (module 761304
                            // walks up to the `<a>`: `href` and its text).
                            .on_mouse_up(MouseButton::Right, {
                                let (url, label) = (attachment.url.clone(), attachment.filename.clone());
                                cx.listener(move |this, event: &gpui::MouseUpEvent, _, _| {
                                    this.menu_hit = Some((
                                        event.position,
                                        super::menu::Hit::Link {
                                            url: url.clone(),
                                            label: label.clone(),
                                        },
                                    ));
                                })
                            })
                            .child(attachment.filename.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color("--text-muted"))
                            .opacity(0.7)
                            .child(super::components::humanize_filesize(attachment.size)),
                    ),
            );

        let (controls, slider_open) = self.audio_controls(&key, &url, salt, true);

        // `hoverButtonGroup_n2rO26` with `nonMediaMosaicItem_n2rO26`: 8 px outside the card's top right corner.
        let hover_button = |id: &'static str, glyph: &'static str, tip: &'static str| {
            let target = div()
                .id((id, salt))
                .flex()
                .p(px(6.))
                .cursor_pointer()
                .text_color(color("--interactive-text-default"))
                .hover(|style| {
                    style
                        .bg(color("--interactive-background-hover"))
                        .text_color(color("--interactive-text-hover"))
                })
                .child(icon(glyph, 20., "--interactive-text-default"));
            (target, tip)
        };
        let mut buttons = div()
            .id(("audio-buttons", salt))
            .absolute()
            .top(px(-10.))
            .right(px(-10.))
            .flex()
            .rounded(px(5.))
            .overflow_hidden()
            .bg(color("--background-base-low"))
            .opacity(0.);
        // While the volume slider is open the group stays hidden (the golden's group is at opacity 0 then).
        if !slider_open {
            buttons = buttons
                .group_hover(group.clone(), |style| style.opacity(1.))
                .hover(|style| style.opacity(1.));
        }
        if own {
            let (target, tip) = hover_button("audio-edit", "clips_pencil", "Modify Attachment");
            let (edit_message, edit_attachment) = (message.clone(), attachment.clone());
            let target = target.on_click(cx.listener(move |this, _, _, cx| {
                this.open_modify_sent(&edit_message, &edit_attachment, cx);
            }));
            buttons = buttons.child(self.with_tip(
                format!("audio-edit-tip-{salt}"),
                Tip::text(tip, Side::Top),
                target,
                cx,
            ));
        }
        if removable {
            let (target, tip) =
                hover_button("audio-remove", "clips_trash", "Remove Message Attachment");
            let (remove_message, remove_attachment) = (message.clone(), attachment.clone());
            let target = target.on_click(cx.listener(move |this, _, _, cx| {
                this.open_remove_sent(&remove_message, &remove_attachment, cx);
            }));
            buttons = buttons.child(self.with_tip(
                format!("audio-remove-tip-{salt}"),
                Tip::text(tip, Side::Top),
                target,
                cx,
            ));
        }
        {
            let (target, tip) = hover_button("audio-download", "clips_download", "Download");
            let target = target.on_click(cx.listener(move |this, _, _, cx| {
                this.download_attachment(&download, cx);
            }));
            buttons = buttons.child(self.with_tip(
                format!("audio-download-tip-{salt}"),
                Tip::text(tip, Side::Top),
                target,
                cx,
            ));
        }

        let (over_hub, over_key, over_url) = (hub.clone(), key.clone(), url);
        div()
            .id(("audio-card", salt))
            .parity("audio-card")
            .group(group)
            .relative()
            .flex()
            .flex_col()
            .w(px(432.))
            .max_w_full()
            .p(px(16.))
            .border_1()
            .border_color(color("--border-subtle"))
            .rounded(px(8.))
            .bg(super::settings::client_theme::solid(
                "--background-base-lower",
            ))
            .on_hover(move |hovered, window, _| {
                let mut hub = over_hub.borrow_mut();
                if *hovered {
                    hub.request(&over_key, &over_url);
                }
                window.refresh();
            })
            .child(metadata)
            .child(controls)
            .child(buttons)
            .into_any_element()
    }

    /// `horizontal_s1cr3c` with `mediaBarInteraction_s1cr3c`: the rail (6 px, `--primary-300` at 30%, 3 px caps at both
    /// ends), the buffered part, the hover preview, the progress, the grabber and the time bubble.
    fn audio_bar(&self, spec: AudioBar<'_>) -> AnyElement {
        let AudioBar {
            key,
            salt,
            span,
            fraction,
            loaded,
            buffered,
            over,
            preview,
            dragging,
            playable,
        } = spec;
        let hub = self.voice_hub.clone();
        let mut rail = color("--primary-300");
        rail.a *= 0.3;
        // A segment of the rail with its caps: `fakeEdges_s1cr3c` puts 3 px round ends outside both sides.
        let segment = |width: f32, tone: gpui::Rgba, alpha: f32| {
            let mut tone = tone;
            tone.a *= alpha;
            div()
                .absolute()
                .top_0()
                .left_0()
                .h_full()
                .w(relative(width))
                .bg(tone)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(-3.))
                        .w(px(3.))
                        .h_full()
                        .rounded_l(px(3.))
                        .bg(tone),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .right(px(-3.))
                        .w(px(3.))
                        .h_full()
                        .rounded_r(px(3.))
                        .bg(tone),
                )
        };
        let bubble_text = if loaded {
            clock(f64::from(preview) * span)
        } else {
            "-:--".to_owned()
        };
        let show = playable && (over || dragging);
        let mut track = div()
            .relative()
            .flex_1()
            .h(px(6.))
            .bg(rail)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(-3.))
                    .w(px(3.))
                    .h_full()
                    .rounded_l(px(3.))
                    .bg(rail),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right(px(-3.))
                    .w(px(3.))
                    .h_full()
                    .rounded_r(px(3.))
                    .bg(rail),
            );
        if buffered {
            track = track.child(segment(1., color("--white"), 0.3));
        }
        track = track
            .child(segment(
                preview,
                color("--white"),
                if show { 0.3 } else { 0. },
            ))
            .child(segment(fraction, color("--brand-500"), 1.));
        // The grabber: 10 px, at the end of the progress, only while the pointer is over the bar.
        let grabber = div()
            .absolute()
            .top(px(-2.))
            .left(relative(fraction))
            .ml(px(-5.))
            .size(px(10.))
            .rounded(px(5.))
            .bg(color("--brand-560"));
        // The bubble: `top: -28px`, centred on the pointer; a 5 px caret under it.
        let bubble = div()
            .absolute()
            .top(px(-28.))
            .left(relative(preview))
            .w(px(0.))
            .flex()
            .justify_center()
            .child(
                div()
                    .flex_none()
                    .relative()
                    .h(px(18.))
                    .px(px(8.))
                    .rounded(px(3.))
                    .bg(color("--black"))
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color("--primary-100"))
                    .child(bubble_text)
                    .child(
                        div().absolute().top_full().left_1_2().ml(px(-5.)).child(
                            canvas(
                                |_, _, _| {},
                                |bounds, _, window, _| {
                                    let mut path = gpui::Path::new(bounds.origin);
                                    path.line_to(point(bounds.origin.x + px(10.), bounds.origin.y));
                                    path.line_to(point(
                                        bounds.origin.x + px(5.),
                                        bounds.origin.y + px(5.),
                                    ));
                                    window.paint_path(path, color("--black"));
                                },
                            )
                            .w(px(10.))
                            .h(px(5.)),
                        ),
                    ),
            );

        let (down_hub, down_key) = (hub.clone(), key.to_owned());
        let (hover_hub, hover_key) = (hub.clone(), key.to_owned());
        let (move_hub, move_key) = (hub.clone(), key.to_owned());
        let (paint_hub, paint_key) = (hub, key.to_owned());
        div()
            .id(("audio-bar", salt))
            .flex()
            .flex_1()
            .min_w(px(0.))
            .h(px(32.))
            .mx(px(7.))
            .items_center()
            .relative()
            .when(playable, move |this| {
                this.cursor_pointer()
                    .on_hover(move |hovered, window, _| {
                        hover_hub.borrow_mut().pill(&hover_key).bar_over = *hovered;
                        window.refresh();
                    })
                    .on_mouse_move(move |event: &MouseMoveEvent, window, _| {
                        let mut hub = move_hub.borrow_mut();
                        let bounds = hub.pill(&move_key).bar;
                        hub.pill(&move_key).preview = fraction_of(event.position.x, bounds) as f32;
                        window.refresh();
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        move |event: &MouseDownEvent, window, _| {
                            // The length is not known before the metadata is in: the bar does nothing yet.
                            if span <= 0. {
                                return;
                            }
                            let mut hub = down_hub.borrow_mut();
                            let bounds = hub.pill(&down_key).bar;
                            hub.drag_start(&down_key);
                            let at = fraction_of(event.position.x, bounds);
                            hub.seek(&down_key, at, span);
                            window.refresh();
                        },
                    )
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds: Bounds<Pixels>, _, window, _| {
                                let mut hub = paint_hub.borrow_mut();
                                hub.pill(&paint_key).bar = bounds;
                                if hub.pill(&paint_key).drag.is_some() {
                                    let (move_hub, move_key) =
                                        (paint_hub.clone(), paint_key.clone());
                                    let (up_hub, up_key) = (paint_hub.clone(), paint_key.clone());
                                    window.on_mouse_event(
                                        move |event: &MouseMoveEvent, phase, window, _| {
                                            if phase != DispatchPhase::Capture {
                                                return;
                                            }
                                            let mut hub = move_hub.borrow_mut();
                                            let bounds = hub.pill(&move_key).bar;
                                            let at = fraction_of(event.position.x, bounds);
                                            hub.pill(&move_key).preview = at as f32;
                                            hub.seek(&move_key, at, span);
                                            window.refresh();
                                        },
                                    );
                                    window.on_mouse_event(
                                        move |_: &MouseUpEvent, phase, window, _| {
                                            if phase != DispatchPhase::Capture {
                                                return;
                                            }
                                            up_hub.borrow_mut().drag_end(&up_key, span);
                                            window.refresh();
                                        },
                                    );
                                }
                            },
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    .child(track)
                    .when(show, |this| this.child(grabber))
                    .when(show, |this| this.child(bubble)),
            )
            .into_any_element()
    }
}

/// Capture only: the steps `au <verb> <part of a file name>`.
#[cfg(feature = "capture")]
impl Root {
    fn audio_target(&self, name: &str) -> Option<(String, Attachment)> {
        let channel = self.channel?;
        self.stores
            .messages
            .get(&channel)?
            .iter()
            .find_map(|message| {
                message
                    .attachments
                    .iter()
                    .find(|attachment| attachment.filename.contains(name))
                    .map(|attachment| {
                        (
                            format!("{KEY}{}:{}", message.id, attachment.id),
                            attachment.clone(),
                        )
                    })
            })
    }

    /// `au play|pause <name>`, `au seek <seconds> <name>`, `au state <name>`.
    pub(crate) fn script_au(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let hub = self.voice_hub.clone();
        let report = match words {
            [verb @ ("play" | "pause"), name] => match self.audio_target(name) {
                Some((key, attachment)) => {
                    let span = match hub.borrow().load(&key) {
                        Some(Load::Ready(clip)) => clip.seconds(),
                        _ => 0.,
                    };
                    let status = voice_player::engine().status();
                    let playing = status.key.as_deref() == Some(key.as_str()) && status.playing;
                    if (*verb == "play") != playing {
                        hub.borrow_mut().toggle(&key, &attachment.url, span);
                    }
                    format!("au: {verb} {}", attachment.filename)
                }
                None => format!("au: no audio attachment named {name}"),
            },
            ["seek", at, name] => match (self.audio_target(name), at.parse::<f64>()) {
                (Some((key, _)), Ok(at)) => {
                    let span = match hub.borrow().load(&key) {
                        Some(Load::Ready(clip)) => clip.seconds(),
                        _ => 0.,
                    };
                    if span > 0. {
                        hub.borrow_mut().seek(&key, at / span, span);
                    }
                    format!("au: seek {at} of {span:.2}")
                }
                _ => "au: seek wants seconds and a name".to_owned(),
            },
            ["state", name] => match self.audio_target(name) {
                Some((key, _)) => {
                    let status = voice_player::engine().status();
                    let load = match hub.borrow().load(&key) {
                        Some(Load::Ready(clip)) => format!("ready {:.3}s", clip.seconds()),
                        Some(Load::Loading) => "loading".to_owned(),
                        Some(Load::Failed) => "failed".to_owned(),
                        None => "not requested".to_owned(),
                    };
                    format!(
                        "au: {load} playing {} position {:.3} key-match {}",
                        status.playing,
                        status.position,
                        status.key.as_deref() == Some(key.as_str())
                    )
                }
                None => format!("au: no audio attachment named {name}"),
            },
            // The shared sound engine and any Play still waiting for a download, whichever channel is open.
            ["engine"] => {
                let status = voice_player::engine().status();
                let waiting = hub.borrow().pills.values().filter(|pill| pill.want_play).count();
                format!(
                    "au: engine key {} playing {} position {:.3}; plays waiting {waiting}",
                    status.key.as_deref().unwrap_or("none"),
                    status.playing,
                    status.position
                )
            }
            ["mute"] => {
                hub.borrow_mut().toggle_mute();
                format!("au: muted {}", hub.borrow().muted)
            }
            _ => "au: usage `au play|pause <name> | seek <s> <name> | state <name> | engine | mute`"
                .to_owned(),
        };
        cx.notify();
        report
    }
}

#[cfg(test)]
mod tests {
    use super::{KEY, clock, clock_pair, embed_audio_key};

    #[test]
    fn the_clock_is_minutes_and_two_digit_seconds_of_the_whole_seconds() {
        assert_eq!(clock(0.), "0:00");
        assert_eq!(clock(3.99), "0:03");
        assert_eq!(clock(8.2), "0:08");
        assert_eq!(clock(61.), "1:01");
        assert_eq!(clock(600.), "10:00");
        assert_eq!(clock(f64::NAN), "0:00");
    }

    #[test]
    fn the_time_reads_dashes_until_the_length_is_known_and_pads_the_elapsed() {
        assert_eq!(
            clock_pair(None, None),
            ("-:--".to_owned(), "-:--".to_owned())
        );
        assert_eq!(
            clock_pair(Some(0.), Some(8.4)),
            ("0:00".to_owned(), "0:08".to_owned())
        );
        assert_eq!(
            clock_pair(Some(3.), Some(600.)),
            ("00:03".to_owned(), "10:00".to_owned())
        );
    }

    #[test]
    fn the_size_is_the_humanize_filesize_of_the_player_metadata() {
        // `Hb` (module 453771) is `humanize.filesize` (796873): the audio card, the video player and the File component
        // all print the same text.
        let size = super::super::components::humanize_filesize;
        assert_eq!(size(35), "35 bytes");
        assert_eq!(size(1023), "1,023 bytes");
        assert_eq!(size(1024), "1.00 KB");
        assert_eq!(size(64_617), "63.10 KB");
        assert_eq!(size(5 * 1024 * 1024), "5.00 MB");
    }

    #[test]
    fn embed_audio_keys_are_deterministic_and_distinct_per_slot_and_url() {
        let (k1, s1) = embed_audio_key(42, 0, "https://media.example/a.mp3");
        let (k2, s2) = embed_audio_key(42, 0, "https://media.example/a.mp3");
        let (k3, s3) = embed_audio_key(42, 1, "https://media.example/a.mp3");
        let (k4, s4) = embed_audio_key(42, 0, "https://media.example/b.mp3");
        assert!(k1.starts_with(KEY));
        assert_eq!((k1.as_str(), s1), (k2.as_str(), s2));
        assert_ne!(k1, k3);
        assert_ne!(s1, s3);
        assert_ne!(k1, k4);
        assert_ne!(s1, s4);
    }
}
