//! The connecting screen (`NM`, module 596339; styles `container_oDjP9R`, measured in
//! `design/dom/loading/connecting.json`): the spinner over a "Did you know" tip, on `--background-base-lowest`, over
//! the whole window. It shows from the first frame of a session until READY is applied and the first content is in,
//! then fades out (`data-fade`: the container over 200ms, the content over 150ms, the spinner scaling to half).
//! After ten seconds the "Connection problems?" links slide up from the bottom (`problems_oDjP9R`).

use super::{Root, SANS, color};
use crate::parity::Parity;
use crate::spinner::{self, Clip};
use gpui::{
    AnyElement, FontWeight, IntoElement, RenderImage, Window, div, img, prelude::*, px, svg,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// `--connecting-container-fade-duration`.
const CONTAINER_FADE: Duration = Duration::from_millis(200);
/// `--connecting-content-fade-duration`.
const CONTENT_FADE: Duration = Duration::from_millis(150);
/// `setProblemsTimeout`.
const PROBLEMS_AFTER: Duration = Duration::from_secs(10);
/// `.full-motion .problems_oDjP9R { transition: transform .2s ease-out }`.
const PROBLEMS_SLIDE: Duration = Duration::from_millis(200);
/// `H.qF7.STATUS` and `H.qF7.TWITTER_SUPPORT` (CONSTANTS.md).
const STATUS_URL: &str = "https://discordstatus.com";
const TWITTER_SUPPORT_URL: &str = "https://twitter.com/discord_support";

/// One piece of a tip: the intl message's text, its `$b` and `$del` rich parts, and its keybind hooks (`NO.e`).
#[derive(Clone, Copy)]
pub(super) enum Seg {
    Text(&'static str),
    Bold(&'static str),
    Strike(&'static str),
    Keys(&'static [&'static str]),
}

/// `_loadingText`: the 59 tips of `NM`, as the en-US catalog words them, in the bundle's order. The last is English
/// only (`locale.startsWith("en-")`). Keybind hooks name the keys of their shortcut (`mod` is ctrl on Windows).
pub(super) const TIPS: &[&[Seg]] = &[
    // Ex79K6
    &[Seg::Text(
        "Pick light or dark theme in Appearance Settings.",
    )],
    // +v5zsT
    &[Seg::Text(
        "Turn off animations with Reduced Motion in Accessibility Settings.",
    )],
    // RLx6+Y
    &[Seg::Text(
        "Change Text-to-Speech speed in Accessibility Settings.",
    )],
    // Q1PZkN
    &[Seg::Text(
        "Make images accessible to everyone. Add a description on upload.",
    )],
    // x7sfVg
    &[Seg::Text(
        "Make roles with unique colors easier to read in Accessibility Settings.",
    )],
    // PHuQsQ
    &[
        Seg::Text("No mouse? Use Keyboard Mode to get around. "),
        Seg::Keys(&["tab"]),
        Seg::Text(" to get started."),
    ],
    // il/GZt
    &[Seg::Text(
        "Adjust color saturation in Accessibility Settings.",
    )],
    // FVVp79
    &[
        Seg::Text("Jump to different sections of Discord with "),
        Seg::Keys(&["f6"]),
        Seg::Text(". "),
        Seg::Keys(&["tab"]),
        Seg::Text(" to get started."),
    ],
    // 5qAKlq
    &[Seg::Text("Scale chat font size in Accessibility Settings.")],
    // XpshBc
    &[Seg::Text(
        "No keyboard? Enable the Send Message button in Appearance Settings to chat.",
    )],
    // 7Y1hFH
    &[Seg::Text(
        "Discord was almost called Bonfire before we picked our name. It was meant to be nice and cozy.",
    )],
    // UcCW71
    &[Seg::Text(
        "Discord was almost called Wyvern before we picked our name. Not too proud of that one.",
    )],
    // E5Ghfc
    &[Seg::Text(
        "There are a bunch of hidden \"Easter Eggs\" in the app that happen when you click certain things...",
    )],
    // bANqo8
    &[Seg::Text(
        "Discord started as a game company making a mobile game called Fates Forever.",
    )],
    // yOEVDr
    &[Seg::Text("Discord’s official birthday is May 13, 2015.")],
    // u0Ra/G
    &[Seg::Text(
        "We came up with the idea of Discord Nitro over morning breakfast potatoes.",
    )],
    // un4cQ7
    &[Seg::Text(
        "Our mascot, Wumpus, was originally created as a character with no friends :(",
    )],
    // 2cyYx1
    &[Seg::Text(
        "In Discord's early days, light theme was the only theme. Dark theme didn't arrive until August 2015.",
    )],
    // KCmze1
    &[Seg::Text(
        "In the ancient days, Discord started as a browser-only app.",
    )],
    // I98MEE
    &[Seg::Text(
        "The character on our 404 page is a robot hamster named Nelly.",
    )],
    // c0YCIx
    &[
        Seg::Text("You can play our version of the Snake game on our 404 page by pressing a "),
        Seg::Strike("secret"),
        Seg::Text(" button."),
    ],
    // z8AvIN
    &[Seg::Text(
        "There's a very small—and we mean small—chance you can get a secret ringtone when calling someone. Good luck!",
    )],
    // IjX3P1
    &[Seg::Text(
        "Our old Partner mascot was an elf named Springle. He recently retired.",
    )],
    // lg3Ckc
    &[Seg::Text(
        "You can use Streamer Mode to hide personal details while streaming.",
    )],
    // 4GaLhY
    &[Seg::Text(
        "You can type /tableflip and /unflip to spice up your messages.",
    )],
    // qHKbUw
    &[Seg::Text(
        "Characters like @, #, !, and * will narrow Quick Switcher results.",
    )],
    // mJxKDw
    &[Seg::Text(
        "Click a server name in the emoji picker to hide that server's emoji.",
    )],
    // iiQBXF
    &[Seg::Text(
        "Hover a GIF and click the star to save it to your favorites.",
    )],
    // YPD46Q
    &[Seg::Text(
        "The top-most role for a user defines that user's color.",
    )],
    // O8Bpga
    &[Seg::Text(
        "A red mic icon means that person has been muted by a server admin.",
    )],
    // xzFwfi
    &[Seg::Text(
        "You can temporarily mute a server or channel by right-clicking it.",
    )],
    // 4G3fsX
    &[Seg::Text(
        "Click your avatar in the lower-left corner to set a custom status.",
    )],
    // /jPyKC
    &[Seg::Text("Group DMs can have up to ten members.")],
    // 8x7D5c
    &[Seg::Text(
        "Click the compass in your server list to find new servers.",
    )],
    // G6Q8H3
    &[Seg::Text(
        "Drag and drop servers on top of each other to create server folders.",
    )],
    // 9Cx+xL
    &[Seg::Text(
        "Type /gif + anything to find a GIF for that topic!",
    )],
    // WsZ8dW
    &[Seg::Text(
        "Share what you're playing by using the Game Activity settings.",
    )],
    // btoe7M
    &[Seg::Text(
        "Highlight text in your chat bar to bold, use italics, and more.",
    )],
    // E6Y0j7
    &[Seg::Text(
        "Hide muted channels in a server by right clicking the server name.",
    )],
    // 7KZ81/
    &[Seg::Text(
        "Customize Discord's appearance in the user settings menu.",
    )],
    // hm7OKu
    &[Seg::Text(
        "Link your favorite social media accounts in the connections settings.",
    )],
    // H535i+
    &[Seg::Text(
        "You can create channel categories to group and organize your channels.",
    )],
    // 08WJUR
    &[Seg::Text(
        "You can join up to 100 servers, and up to 200 with Nitro!",
    )],
    // hhtfyi
    &[Seg::Text(
        "You can drag and drop files onto Discord to upload them.",
    )],
    // on980U
    &[Seg::Text(
        "Change each participant's volume by right-clicking them in a call.",
    )],
    // AL6c01
    &[Seg::Text(
        "Right click to pin messages in a channel or DM to save them for later.",
    )],
    // iLTeF6
    &[Seg::Text(
        "Type a plus sign before an emoji name to turn it into a reaction.",
    )],
    // b3vSu7
    &[Seg::Text(
        "You can type /nick to quickly change your nickname in a server.",
    )],
    // f8ao58
    &[Seg::Text(
        "You can type / to view bot commands and other built-in commands",
    )],
    // 1v1h8o
    &[
        Seg::Text("You can type "),
        Seg::Text("**"),
        Seg::Text(" around your words to make them "),
        Seg::Bold("bold"),
        Seg::Text("."),
    ],
    // MUlAVW
    &[
        Seg::Keys(&["ctrl", "k"]),
        Seg::Text(" to quickly find a previous conversation or channel."),
    ],
    // aes5VI
    &[
        Seg::Keys(&["alt", "click"]),
        Seg::Text(" a message to mark it as unread."),
    ],
    // md9Svv
    &[
        Seg::Keys(&["shift", "esc"]),
        Seg::Text(" to mark an entire server as read."),
    ],
    // X0Tu+u
    &[
        Seg::Keys(&["shift", "alt", "up"]),
        Seg::Text(" or "),
        Seg::Keys(&["down"]),
        Seg::Text(" will navigate between unread channels"),
    ],
    // AkoeIY
    &[
        Seg::Text("Use "),
        Seg::Keys(&["ctrl", "/"]),
        Seg::Text(" to bring up the list of keyboard shortcuts."),
    ],
    // P9sfAH
    &[
        Seg::Keys(&["shift", "enter"]),
        Seg::Text(" to make a new line without sending your message."),
    ],
    // 3Ox33f
    &[
        Seg::Text("Holding "),
        Seg::Keys(&["shift"]),
        Seg::Text(" while clicking emoji allows you to send multiple emoji."),
    ],
    // enCV6b
    &[
        Seg::Text("You can press "),
        Seg::Keys(&["up"]),
        Seg::Text(" to quickly edit your most recent message."),
    ],
    // dQ9Wqk
    &[Seg::Text(
        "Statistically speaking, the majority of Discord users believe that pineapple and pizza is the most cursed food combo. Thank our Polls feature launch for this Discord Fact™.",
    )],
];

/// The screen's own state. Frames of the spinner arrive once the clip is fetched and decoded.
pub(super) struct Splash {
    started: Instant,
    /// When the first content was in; the fade runs from here.
    loaded_at: Option<Instant>,
    /// The screen is gone: it faded out, and its frames are dropped.
    done: bool,
    /// A capture holds the screen up (`splash hold`), whatever has loaded.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    hold: bool,
    tip: usize,
    frames: Vec<Arc<RenderImage>>,
    /// The width of a space in the tip's 16px text, for tips that flow as words.
    space: Option<f32>,
}

impl Splash {
    pub(super) fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.subsec_nanos() as usize);
        Self {
            started: Instant::now(),
            loaded_at: None,
            done: false,
            hold: false,
            tip: nanos % TIPS.len(),
            frames: Vec::new(),
            space: None,
        }
    }
}

/// `cubic-bezier(0, 0, .58, 1)`, CSS `ease-out`, at time fraction `x`.
pub(super) fn ease_out(x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    let (x1, y1, x2, y2) = (0.0_f32, 0.0_f32, 0.58_f32, 1.0_f32);
    let at = |t: f32, a: f32, b: f32| {
        let u = 1. - t;
        3. * u * u * t * a + 3. * u * t * t * b + t * t * t
    };
    let slope = |t: f32| {
        let u = 1. - t;
        3. * u * u * x1 + 6. * u * t * (x2 - x1) + 3. * t * t * (1. - x2)
    };
    let mut t = x;
    for _ in 0..8 {
        let error = at(t, x1, x2) - x;
        let rate = slope(t);
        if rate.abs() < 1e-6 {
            break;
        }
        t = (t - error / rate).clamp(0., 1.);
    }
    at(t, y1, y2)
}

/// How far a `duration` fade has run `since` its start, 0 to 1, eased.
fn fade(since: Duration, duration: Duration) -> f32 {
    ease_out(since.as_secs_f32() / duration.as_secs_f32())
}

/// What the screen draws at `elapsed` since it appeared and `faded` since the content was in.
#[derive(Debug, PartialEq)]
pub(super) struct Look {
    pub container: f32,
    pub content: f32,
    pub spinner_scale: f32,
    pub spinner_opacity: f32,
    /// Slide of the problems panel: 1 below the window, 0 in place.
    pub problems: f32,
    pub gone: bool,
}

pub(super) fn look(elapsed: Duration, faded: Option<Duration>, hold: bool) -> Look {
    let slid = if elapsed >= PROBLEMS_AFTER {
        1. - fade(elapsed - PROBLEMS_AFTER, PROBLEMS_SLIDE)
    } else {
        1.
    };
    let Some(since) = faded.filter(|_| !hold) else {
        return Look {
            container: 1.,
            content: 1.,
            spinner_scale: 1.,
            spinner_opacity: 1.,
            problems: slid,
            gone: false,
        };
    };
    Look {
        container: 1. - fade(since, CONTAINER_FADE),
        content: 1. - fade(since, CONTENT_FADE),
        spinner_scale: 1. - 0.5 * fade(since, CONTENT_FADE),
        spinner_opacity: 1. - fade(since, CONTENT_FADE),
        problems: slid,
        gone: since >= CONTAINER_FADE,
    }
}

/// One key of a keybind hook (`NO.e`): `.key_dMUH0f`, upper-cased by `text-transform`; arrows are the 10px glyph.
fn keycap(key: &'static str) -> AnyElement {
    div()
        .flex_none()
        .h(px(23.))
        .min_w(px(14.))
        .px(px(6.))
        .pt(px(3.))
        .pb(px(4.))
        .mr(px(3.))
        .rounded(px(4.))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-mod-muted"))
        .text_color(color("--interactive-text-active"))
        .text_size(px(12.))
        .line_height(px(12.))
        .font_weight(FontWeight(600.))
        .flex()
        .items_center()
        .justify_center()
        .child(match key {
            "up" | "down" => svg()
                .path("icons/key_arrow.svg")
                .size(px(10.))
                .flex_none()
                .text_color(color("--interactive-text-active"))
                .when(key == "up", |this| {
                    this.with_transformation(gpui::Transformation::rotate(gpui::radians(
                        std::f32::consts::PI,
                    )))
                })
                .into_any_element(),
            other => other.to_uppercase().into_any_element(),
        })
        .into_any_element()
}

/// The tip, 16px / 20px, centred in 300px. A plain tip is one wrapped text; one with keycaps or rich parts flows as
/// words so the keycaps sit inline.
fn tip_body(tip: &'static [Seg], space: f32) -> AnyElement {
    let base = div()
        .max_w(px(300.))
        .text_size(px(16.))
        .line_height(px(20.))
        .text_color(color("--text-default"))
        .text_center();
    if let [Seg::Text(text)] = tip {
        return base.child(*text).into_any_element();
    }
    let mut words: Vec<AnyElement> = Vec::new();
    for seg in tip {
        match seg {
            Seg::Text(text) | Seg::Bold(text) | Seg::Strike(text) => {
                let pieces: Vec<&str> = text.split_whitespace().collect();
                let last = pieces.len().saturating_sub(1);
                for (index, word) in pieces.iter().enumerate() {
                    // A word is followed by a space unless the text ends on it without one (it joins the next piece).
                    let spaced = index < last || text.ends_with(char::is_whitespace);
                    let piece = div()
                        .flex_none()
                        .mr(px(if spaced { space } else { 0. }))
                        .child(*word);
                    words.push(match seg {
                        Seg::Bold(_) => piece.font_weight(FontWeight(700.)).into_any_element(),
                        Seg::Strike(_) => piece.line_through().into_any_element(),
                        _ => piece.into_any_element(),
                    });
                }
            }
            Seg::Keys(keys) => {
                for key in *keys {
                    words.push(keycap(key));
                }
            }
        }
    }
    base.flex()
        .flex_wrap()
        .justify_center()
        .children(words)
        .into_any_element()
}

fn measure(window: &mut Window, text: &str, size: f32) -> f32 {
    let mut run_font: gpui::Font = gpui::font(SANS);
    run_font.weight = FontWeight(400.);
    run_font.features = super::kerning();
    let run = gpui::TextRun {
        len: text.len(),
        font: run_font,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    f32::from(
        window
            .text_system()
            .shape_line(
                gpui::SharedString::from(text.to_owned()),
                px(size),
                &[run],
                None,
            )
            .width,
    )
}

/// A space in the 16px text: "a a" less "aa".
fn space_width(window: &mut Window) -> f32 {
    measure(window, "a a", 16.) - measure(window, "aa", 16.)
}

/// A link of the problems panel: `links_oDjP9R`, 16px medium, the icon 16px with 4px after it.
fn problem_link(
    id: &'static str,
    glyph: &'static str,
    label: &'static str,
    url: &'static str,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .h(px(16.))
        .text_size(px(16.))
        .line_height(px(16.))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-link"))
        .cursor_pointer()
        .hover(|style| style.opacity(0.8))
        .on_click(move |_, _, cx| cx.open_url(url))
        .child(
            svg()
                .path(format!("icons/{glyph}.svg"))
                .size(px(16.))
                .flex_none()
                .mr(px(4.))
                .text_color(color("--text-link")),
        )
        .child(label)
}

impl Root {
    pub(crate) fn ready_for_capture(&self) -> bool {
        self.ready && self.splash.done
    }

    /// The spinner's frames came in: gpui images, drawn at 200px.
    pub(super) fn splash_clip(&mut self, clip: &Clip) {
        if self.splash.done && !cfg!(feature = "capture") {
            return;
        }
        self.splash.frames = clip
            .frames
            .iter()
            .filter_map(|bgra| {
                let buffer = image::RgbaImage::from_raw(clip.width, clip.height, bgra.clone())?;
                Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
            })
            .collect();
    }

    /// Whether the first content is in: READY is applied, and the channel it opened has its first messages (or none
    /// was opened).
    pub(super) fn content_ready(&self) -> bool {
        self.stores.current_user.is_some() && (self.channel.is_none() || self.ready)
    }

    /// Called after every update: starts the fade once the content is ready.
    pub(super) fn splash_check(&mut self) {
        if self.splash.loaded_at.is_none() && self.content_ready() {
            self.splash.loaded_at = Some(Instant::now());
        }
    }

    /// `splash hold`, `splash release` and `splash tip <n>` of the capture harness.
    #[cfg(feature = "capture")]
    pub(crate) fn script_splash(&mut self, words: &[&str]) -> String {
        match words {
            ["hold"] => {
                self.splash.done = false;
                self.splash.hold = true;
                "splash held".to_owned()
            }
            ["release"] => {
                self.splash.hold = false;
                "splash released".to_owned()
            }
            ["tip", index] => {
                if let Ok(index) = index.parse::<usize>() {
                    self.splash.tip = index % TIPS.len();
                }
                format!("splash tip {}", self.splash.tip)
            }
            _ => "usage: splash hold | release | tip <n>".to_owned(),
        }
    }

    pub(super) fn splash_layer(&mut self, window: &mut Window) -> Option<AnyElement> {
        if self.splash.done {
            return None;
        }
        let elapsed = self.splash.started.elapsed();
        let faded = self.splash.loaded_at.map(|at| at.elapsed());
        let look = look(elapsed, faded, self.splash.hold);
        if look.gone {
            self.splash.done = true;
            if !cfg!(feature = "capture") {
                self.splash.frames = Vec::new();
            }
            return None;
        }
        // The spinner and the slide are animations: ask for the next frame.
        window.request_animation_frame();
        let space = *self.splash.space.get_or_insert_with(|| space_width(window));
        let frame = (!self.splash.frames.is_empty()).then(|| {
            let at = (elapsed.as_secs_f32() * spinner::FPS) as usize % self.splash.frames.len();
            self.splash.frames[at].clone()
        });
        let tip = TIPS[self.splash.tip];
        // The content is 292px tall, centred in the window less the 22px the official bar keeps. The tip block rides
        // 20px up (`top: -20px`) from where the inline video's line box leaves it, 3px below the spinner.
        let content = div()
            .parity("loading-content")
            .w(px(300.))
            .h(px(292.328))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .parity("loading-spinner")
                    .size(px(200.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(look.spinner_opacity)
                    .children(frame.map(|frame| {
                        img(frame)
                            .size(px(200. * look.spinner_scale))
                            .into_any_element()
                    })),
            )
            .child(
                div()
                    .w(px(300.))
                    .mt(px(-20. + 3.))
                    .opacity(look.content)
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .parity("loading-tip-title")
                            .w(px(300.))
                            .h(px(21.328))
                            .mb(px(8.))
                            .text_size(px(16.))
                            .line_height(px(21.3333))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-subtle"))
                            .text_center()
                            .child("Did you know"),
                    )
                    .child(div().parity("loading-tip").child(tip_body(tip, space))),
            );
        // `problems_oDjP9R`: 71.23px tall (39.23 of content, 32 below), pushed below the window until it slides in.
        let problems = div()
            .absolute()
            .left(px(0.))
            .right(px(0.))
            .bottom(px(-71.23 * look.problems))
            .pb(px(32.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(14.))
                    .mb(px(8.))
                    .text_color(color("--text-default"))
                    .child("Connection problems? Let us know!"),
            )
            .child(
                div()
                    .flex()
                    .child(problem_link(
                        "loading-twitter",
                        "loading_x",
                        "Tweet Us",
                        TWITTER_SUPPORT_URL,
                    ))
                    .child(div().ml(px(20.)).child(problem_link(
                        "loading-status",
                        "loading_status",
                        "Server Status",
                        STATUS_URL,
                    ))),
            );
        Some(
            div()
                .id("loading-screen")
                .parity("loading-container")
                .absolute()
                .top(px(-22.))
                .left(px(0.))
                .w_full()
                .h(window.viewport_size().height + px(22.))
                .overflow_hidden()
                .bg(color("--background-base-lowest"))
                .opacity(look.container)
                .flex()
                .items_center()
                .justify_center()
                .pt(px(22.))
                // The screen is above everything and takes the pointer.
                .occlude()
                .child(content)
                .child(problems)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_out_runs_from_zero_to_one_and_leads_a_straight_line() {
        assert_eq!(ease_out(0.), 0.);
        assert!((ease_out(1.) - 1.).abs() < 1e-4);
        assert!(ease_out(0.5) > 0.5);
        assert!(ease_out(0.25) < ease_out(0.75));
    }

    #[test]
    fn the_screen_holds_until_the_content_is_in_then_fades_in_200ms() {
        let up = look(Duration::from_secs(2), None, false);
        assert_eq!((up.container, up.content, up.gone), (1., 1., false));
        let half = look(
            Duration::from_secs(3),
            Some(Duration::from_millis(75)),
            false,
        );
        assert!(half.container < 1. && half.container > 0.);
        assert!(
            half.content < half.container,
            "the 150ms content fades faster"
        );
        assert!(half.spinner_scale < 1. && half.spinner_scale > 0.5);
        let end = look(
            Duration::from_secs(3),
            Some(Duration::from_millis(200)),
            false,
        );
        assert!(end.gone);
        assert!(end.container.abs() < 1e-4);
        // A capture's hold keeps it up whatever loaded.
        assert!(!look(Duration::from_secs(3), Some(Duration::from_secs(5)), true).gone);
    }

    #[test]
    fn the_problems_slide_in_after_ten_seconds() {
        assert_eq!(look(Duration::from_secs(9), None, false).problems, 1.);
        assert!(look(Duration::from_millis(10_300), None, false).problems < 1e-4);
        let between = look(Duration::from_millis(10_050), None, false).problems;
        assert!(between < 1. && between > 0.);
    }

    #[test]
    fn tips_are_the_bundles_fifty_nine_and_each_has_words() {
        assert_eq!(TIPS.len(), 59);
        assert!(TIPS.iter().all(|tip| !tip.is_empty()));
        assert!(
            TIPS.iter()
                .any(|tip| matches!(tip, [Seg::Text(text)] if text.contains("secret ringtone")))
        );
        // Hooked tips keep their keys: "mod+k" is ctrl and k.
        assert!(TIPS.iter().any(|tip| {
            tip.iter()
                .any(|seg| matches!(seg, Seg::Keys(["ctrl", "k"])))
        }));
    }
}
