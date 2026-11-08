//! The voice panel's popouts over its status row: the connection popout (module 926641, the `RTCConnection` stats and
//! Upload Logs, and the Privacy tab with the call's code) and the noise suppression popout (`TL` in module 596339).
//! Measured in `design/dom/popouts/voice-panel-ping-darker-cozy.json` and `voice-panel-krisp-darker-cozy.json`.
//!
//! The official noise popout names Krisp. gravecord does not run Krisp (BLOCKED.md): the same popout names what runs,
//! DeepFilterNet, the owner's decision in DECISIONS.md.

use super::{Root, color, icon, tooltip};
use dc_state::Id;
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, IntoElement, Window, deferred, div,
    linear_color_stop, linear_gradient, prelude::*, px,
};
use std::time::{Duration, Instant};

const TROUBLESHOOTING: &str = "https://support.discord.com/hc/en-us/articles/360045138471";
const NOISE_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/360040843952";
const ENCRYPTION_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/25968222946071";
const DEEP_FILTER_NET: &str = "https://github.com/Rikorose/DeepFilterNet";

/// `_pings` keeps fewer than this many.
const PINGS_KEPT: usize = 200;
/// `getAveragePing` looks at the first twenty.
const PINGS_AVERAGED: usize = 20;
/// The strings' `badPing` and `badLossRate`.
const BAD_PING: u32 = 250;
const BAD_LOSS: u32 = 10;
/// A popout closed by a press on its own button stays closed for the click that follows.
const REOPEN_GUARD: Duration = Duration::from_millis(400);
/// `noiseCancellationPopout_9U6LUN`.
const NOISE_WIDTH: f32 = 276.;
/// The popouts' bottom edge above the window's: 8px over the status row.
const POPOUT_BOTTOM: f32 = 162.;

/// What the connection has measured (`RTCConnection`'s `_pings` and `_outboundLossRate`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CallStats {
    pings: Vec<f64>,
    loss: Option<f32>,
    /// The DAVE epoch's voice privacy code, thirty digits.
    privacy_code: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Quality {
    Unknown,
    Fine,
    Average,
    Bad,
}

impl CallStats {
    /// `_handlePing`.
    pub fn ping(&mut self, ms: f64) {
        self.pings.push(ms);
        while self.pings.len() >= PINGS_KEPT {
            self.pings.remove(0);
        }
    }

    pub fn set_loss(&mut self, percent: f32) {
        self.loss = Some(percent);
    }

    pub fn set_privacy_code(&mut self, code: String) {
        self.privacy_code = Some(code);
    }

    /// `getAveragePing`: of the first twenty kept, as the official slice reads.
    pub fn average(&self) -> f64 {
        let first = &self.pings[..self.pings.len().min(PINGS_AVERAGED)];
        if first.is_empty() {
            return 0.;
        }
        first.iter().sum::<f64>() / first.len() as f64
    }

    pub fn last(&self) -> Option<f64> {
        self.pings.last().copied()
    }

    /// `get quality`.
    pub fn quality(&self, connected: bool) -> Quality {
        let Some(last) = self.last().filter(|_| connected) else {
            return Quality::Unknown;
        };
        let loss = self.loss.unwrap_or(0.);
        if last > 500. || loss > 10. {
            Quality::Bad
        } else if last > 250. || loss > 5. {
            Quality::Average
        } else {
            Quality::Fine
        }
    }
}

impl Quality {
    /// `largePing` over a quality class; an unknown quality reads as fine, as the panel draws it while connecting.
    pub fn background(self) -> &'static str {
        match self {
            Quality::Unknown | Quality::Fine => "--background-feedback-mod-positive",
            Quality::Average => "--background-feedback-mod-warning",
            Quality::Bad => "--background-feedback-mod-critical",
        }
    }
    pub fn text(self) -> &'static str {
        match self {
            Quality::Unknown | Quality::Fine => "--text-feedback-positive",
            Quality::Average => "--text-feedback-warning",
            Quality::Bad => "--text-feedback-critical",
        }
    }
}

/// The troubleshooting line under the numbers (`3pFz1P`, or `vggaMt` while no loss is known), split around its link.
fn advice(loss_known: bool) -> (String, &'static str, &'static str) {
    let before = if loss_known {
        format!(
            "You may notice delayed audio at {BAD_PING} ms or higher. You may sound robotic if your packet loss rate \
             is over {BAD_LOSS}%. If the problem persists, disconnect and try again. See our "
        )
    } else {
        format!(
            "You may notice delayed audio at {BAD_PING} ms or higher. If the problem persists, disconnect and try \
             again. See our "
        )
    };
    (before, "troubleshooting guide", " for more assistance.")
}

/// The code in chunks of five (`chunkSize: 5, desiredLength: 30`).
fn code_chunks(code: &str) -> Vec<String> {
    code.as_bytes()
        .chunks(5)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    Connection,
    Privacy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Popout {
    Connection(Tab),
    Noise,
}

#[derive(Default)]
pub(crate) struct PanelState {
    pub popout: Option<Popout>,
    closed_at: Option<(Popout, Instant)>,
    /// Upload Logs: `uploading`, then `success` for two seconds.
    upload_done_at: Option<Instant>,
    uploading: bool,
    /// Copy, then Copied for two seconds.
    copied_at: Option<Instant>,
}

fn same_kind(a: Popout, b: Popout) -> bool {
    matches!(
        (a, b),
        (Popout::Connection(_), Popout::Connection(_)) | (Popout::Noise, Popout::Noise)
    )
}

impl Root {
    /// A press on the popout's own button closed it a moment ago: that press's click must not open it again.
    pub(super) fn toggle_panel_popout(&mut self, popout: Popout, cx: &mut Context<Self>) {
        let panel = &mut self.panel;
        let just_closed = panel
            .closed_at
            .is_some_and(|(closed, at)| same_kind(closed, popout) && at.elapsed() < REOPEN_GUARD);
        panel.popout = match panel.popout {
            Some(open) if same_kind(open, popout) => None,
            _ if just_closed => None,
            _ => Some(popout),
        };
        cx.notify();
    }

    fn close_panel_popout(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = self.panel.popout.take() {
            self.panel.closed_at = Some((open, Instant::now()));
            cx.notify();
        }
    }

    /// The noise popout is open: the microphone meters and Mic Test run as on the Voice & Video page.
    pub(in crate::ui) fn noise_popout_open(&self) -> bool {
        self.panel.popout == Some(Popout::Noise)
    }

    /// The logs were sent (`voice-logs-upload`): the button reads Done for a moment.
    pub(in crate::ui) fn panel_logs_uploaded(&mut self, cx: &mut Context<Self>) {
        if !self.panel.uploading {
            return;
        }
        self.panel.uploading = false;
        self.panel.upload_done_at = Some(Instant::now());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                this.panel.upload_done_at = None;
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn voice_panel_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.call.is_none() {
            self.panel.popout = None;
        }
        let body = match self.panel.popout? {
            Popout::Connection(tab) => self.connection_popout(tab, cx),
            Popout::Noise => self.noise_popout(window, cx),
        };
        Some(deferred(body).with_priority(3).into_any_element())
    }

    /// `popover_81IzEA` at the ping, 300 wide.
    fn connection_popout(&mut self, tab: Tab, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref();
        let connected = call.is_some_and(|call| call.connected);
        let stats = call.map(|call| call.stats.clone()).unwrap_or_default();
        let others = call.is_some_and(|call| {
            call.has_others(self.stores.current_user.as_ref().map(|user| user.id))
        });
        // `k`: the negotiated DAVE version is not 0. This client only completes a connection on DAVE.
        let encrypted = connected;
        let tab = if others && encrypted {
            tab
        } else {
            Tab::Connection
        };
        let tab_item = |id: &'static str, label: &'static str, this_tab: Tab| {
            let selected = tab == this_tab;
            div()
                .id(id)
                .relative()
                .h(px(30.))
                .mb(px(-1.))
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(14.))
                .font_weight(FontWeight(600.))
                .text_color(color(if selected {
                    "--text-brand"
                } else {
                    "--text-muted"
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.panel.popout = Some(Popout::Connection(this_tab));
                    cx.notify();
                }))
                .child(label)
                .when(selected, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(2.))
                            .rounded_t(px(2.))
                            .bg(color("--text-brand")),
                    )
                })
        };
        let connection_tab = tab_item("voice-tab-connection", "Connection", Tab::Connection);
        let privacy_tab = tab_item("voice-tab-privacy", "Privacy", Tab::Privacy);
        let separator = || div().h(px(1.)).bg(color("--border-subtle"));
        let banner = |text: &'static str| {
            div()
                .h(px(24.))
                .px(px(8.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .gap(px(4.))
                .bg(color("--background-feedback-mod-positive"))
                .child(icon(
                    "voice_panel_end_to_end_encrypted",
                    12.,
                    "--text-feedback-positive",
                ))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-feedback-positive"))
                        .child(text),
                )
        };
        let content: AnyElement = match tab {
            Tab::Connection => {
                let stat = |label: &str, value: String| {
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-default"))
                        .flex()
                        .child(label.to_owned())
                        .child(div().font_weight(FontWeight(600.)).child(value))
                };
                let (before, link, after) = advice(stats.loss.is_some());
                let uploading = self.panel.uploading;
                let done = self.panel.upload_done_at.is_some();
                div()
                    .h(px(251.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(stat(
                                        "Average ping: ",
                                        format!("{:.0} ms", stats.average()),
                                    ))
                                    .children(
                                        stats.last().map(|last| {
                                            stat("Last ping: ", format!("{last:.0} ms"))
                                        }),
                                    )
                                    .children(stats.loss.map(|loss| {
                                        stat("Outbound packet loss rate: ", format!("{loss:.1}%"))
                                    })),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(self.text_with_link(
                                        "voice-panel-guide",
                                        &before,
                                        link,
                                        after,
                                        cx,
                                        |_, cx| cx.open_url(TROUBLESHOOTING),
                                    )),
                            )
                            .child(separator())
                            .child(
                                div()
                                    .id("voice-panel-upload")
                                    .h(px(32.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--control-secondary-border-default"))
                                    .bg(color("--control-secondary-background-default"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(4.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .when(!uploading && !done, |this| {
                                        this.cursor_pointer()
                                            .hover(|style| {
                                                style.bg(color(
                                                    "--control-secondary-background-hover",
                                                ))
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.panel.uploading = true;
                                                this.voice_upload_logs(cx);
                                                cx.notify();
                                            }))
                                    })
                                    .when(!uploading, |this| {
                                        this.child(icon(
                                            "voice_panel_upload_logs",
                                            16.,
                                            "--control-secondary-text-default",
                                        ))
                                    })
                                    .child(if done {
                                        "Done"
                                    } else if uploading {
                                        "Uploading"
                                    } else {
                                        "Upload Logs"
                                    }),
                            ),
                    )
                    .child(banner(if encrypted {
                        "End-to-end encrypted"
                    } else {
                        "Secure Connection"
                    }))
                    .into_any_element()
            }
            Tab::Privacy => {
                let code = stats.privacy_code.clone().unwrap_or_default();
                let chunks = code_chunks(&code);
                let copied = self.panel.copied_at.is_some();
                div()
                    .h(px(251.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("Voice Privacy Code"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(self.text_with_link(
                                "voice-panel-e2ee",
                                "A new code is generated when people join or leave this call. ",
                                "Learn more about end-to-end encryption.",
                                "",
                                cx,
                                |_, cx| cx.open_url(ENCRYPTION_ARTICLE),
                            )),
                    )
                    .child(
                        div()
                            .h(px(104.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lower"))
                            .p(px(8.))
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .justify_center()
                            .children(chunks.into_iter().map(|chunk| {
                                div()
                                    .w(px(84.))
                                    .text_center()
                                    .text_size(px(14.))
                                    .line_height(px(22.))
                                    .font_family("Consolas")
                                    .text_color(color("--text-default"))
                                    .child(chunk)
                            })),
                    )
                    .child(
                        div()
                            .id("voice-panel-copy")
                            .h(px(32.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-secondary-border-default"))
                            .bg(color("--control-secondary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let spaced = code_chunks(&code).join(" ");
                                cx.write_to_clipboard(ClipboardItem::new_string(spaced));
                                this.panel.copied_at = Some(Instant::now());
                                cx.spawn(async move |this, cx| {
                                    cx.background_executor().timer(Duration::from_secs(2)).await;
                                    let _ = this.update(cx, |this, cx| {
                                        this.panel.copied_at = None;
                                        cx.notify();
                                    });
                                })
                                .detach();
                                cx.notify();
                            }))
                            .child(if copied { "Copied" } else { "Copy" }),
                    )
                    .child(separator())
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-subtle"))
                            .child(
                                "Only you and your friends on this call can hear you. Nobody else \u{2014} not \
                                 even Wumpus \u{2014} can listen in!",
                            ),
                    )
                    .child(banner("End-to-end encrypted"))
                    .into_any_element()
            }
        };
        div()
            .id("voice-connection-popout")
            .absolute()
            .left(px(20.))
            .bottom(px(POPOUT_BOTTOM))
            .w(px(300.))
            .p(px(15.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_panel_popout(cx)))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .gap(px(24.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .child(connection_tab)
                    .when(others && encrypted, |this| this.child(privacy_tab)),
            )
            .child(content)
            .into_any_element()
    }

    /// `noiseCancellationPopout_9U6LUN`: 276 wide, centred over the button.
    fn noise_popout(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        const DESCRIPTION: &str = "Enable DeepFilterNet noise suppression! Make some noise\u{2014}like clapping your \
                                   hands\u{2014}while speaking. Your friends will hear nothing but your beautiful \
                                   voice.";
        let on = self.noise_cancellation();
        let locked = self.noise_cancellation_locked();
        let label = |text: &'static str| {
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let switch = div()
            .id("voice-noise-switch")
            .child(if locked {
                super::create_channel::switch_dimmed(on, "--background-surface-higher")
            } else {
                super::create_channel::switch(on)
            })
            .when(!locked, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_noise_cancellation(!on);
                        cx.notify();
                    }))
            });
        let switch: AnyElement = match self.voice_profile_label().filter(|_| locked) {
            Some(profile) => self
                .with_tip(
                    "voice-noise-switch-tip",
                    tooltip::Tip::body(
                        tooltip::Body::Text(
                            format!(
                                "Noise cancellation is {} by the {profile} input profile.",
                                if on { "enabled" } else { "disabled" }
                            )
                            .into(),
                        ),
                        tooltip::Side::Top,
                    ),
                    switch,
                    cx,
                )
                .into_any_element(),
            None => switch.into_any_element(),
        };
        let test_button = self.small_mic_test(window, cx);
        div()
            .id("voice-noise-popout")
            .absolute()
            .left(px(164.))
            .bottom(px(POPOUT_BOTTOM))
            .w(px(NOISE_WIDTH))
            .p(px(15.))
            .rounded(px(4.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_panel_popout(cx)))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label("Noise Suppression"))
                    .child(switch),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(DESCRIPTION),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(label("Mic Test"))
                    .child(test_button),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(label("Powered by"))
                    .child(
                        div()
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .id("voice-noise-logo")
                                    .cursor_pointer()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .on_click(|_, _, cx| cx.open_url(DEEP_FILTER_NET))
                                    .child("DeepFilterNet"),
                            )
                            .child(
                                div()
                                    .id("voice-noise-learn-more")
                                    .cursor_pointer()
                                    .text_size(px(14.))
                                    .line_height(px(14.))
                                    .text_color(color("--text-link"))
                                    .hover(|style| style.underline())
                                    .on_click(|_, _, cx| cx.open_url(NOISE_ARTICLE))
                                    .child("Learn More"),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// `micTest_lUTBfB small_lUTBfB`: a small Test or Stop button and the 176 by 20 meter, on the popout's colour.
    fn small_mic_test(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        const METER: f32 = 176.;
        let testing = self.voice_mic_testing();
        let progress = self.voice_mic_progress();
        let shift = (progress / 100.) * METER;
        let notches = self.small_notches(METER as u32, 20);
        let button = div()
            .id("voice-noise-test")
            .w(px(60.))
            .h(px(32.))
            .flex_none()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-secondary-border-default"))
            .bg(color("--control-secondary-background-default"))
            .cursor_pointer()
            .hover(|style| style.bg(color("--control-secondary-background-hover")))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-secondary-text-default"))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_mic_test(cx)))
            .child(if testing { "Stop" } else { "Test" });
        let button: AnyElement =
            if self.call.as_ref().is_some_and(|call| call.connected) && !testing {
                self.with_tip(
                    "voice-noise-test-tip",
                    tooltip::Tip::text(
                        "You'll be muted and deafened in your voice session during Mic Testing.",
                        tooltip::Side::Top,
                    ),
                    button,
                    cx,
                )
                .into_any_element()
            } else {
                button.into_any_element()
            };
        let meter = div()
            .relative()
            .w(px(METER))
            .h(px(20.))
            .overflow_hidden()
            .bg(color("--background-mod-strong"))
            .when(progress > 0., |this| {
                this.child(div().absolute().top_0().left_0().h_full().w(px(shift)).bg(
                    linear_gradient(
                        90.,
                        linear_color_stop(color("--yellow-260"), 0.),
                        linear_color_stop(color("--green-360"), 1.),
                    ),
                ))
            })
            .child(
                gpui::img(notches)
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(METER))
                    .h(px(20.)),
            );
        div()
            .h(px(40.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(button)
            .child(meter)
            .into_any_element()
    }

    /// The panel's channel name goes to the call's channel.
    pub(in crate::ui) fn panel_channel_clicked(
        &mut self,
        guild: Id,
        channel: Id,
        cx: &mut Context<Self>,
    ) {
        self.select(guild, Some(channel));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_average_reads_the_first_twenty_and_the_list_stays_under_two_hundred() {
        let mut stats = CallStats::default();
        assert_eq!(stats.average(), 0.);
        assert_eq!(stats.last(), None);
        for ms in 0..30 {
            stats.ping(f64::from(ms));
        }
        assert_eq!(stats.average(), 9.5);
        assert_eq!(stats.last(), Some(29.));
        for _ in 0..400 {
            stats.ping(1.);
        }
        assert_eq!(stats.pings.len(), PINGS_KEPT - 1);
    }

    #[test]
    fn quality_follows_the_official_cutoffs() {
        let mut stats = CallStats::default();
        assert_eq!(stats.quality(true), Quality::Unknown);
        stats.ping(100.);
        assert_eq!(stats.quality(false), Quality::Unknown);
        assert_eq!(stats.quality(true), Quality::Fine);
        stats.ping(251.);
        assert_eq!(stats.quality(true), Quality::Average);
        stats.ping(501.);
        assert_eq!(stats.quality(true), Quality::Bad);
        stats.ping(100.);
        stats.set_loss(6.);
        assert_eq!(stats.quality(true), Quality::Average);
        stats.set_loss(11.);
        assert_eq!(stats.quality(true), Quality::Bad);
    }

    #[test]
    fn the_advice_names_the_loss_rate_only_when_it_is_known() {
        assert!(advice(true).0.contains("packet loss rate is over 10%"));
        assert!(!advice(false).0.contains("packet loss"));
        assert!(advice(false).0.contains("250 ms"));
    }

    #[test]
    fn the_code_reads_in_chunks_of_five() {
        assert_eq!(code_chunks("1234567890"), vec!["12345", "67890"]);
        assert!(code_chunks("").is_empty());
    }

    #[test]
    fn a_press_that_closed_a_popout_does_not_count_for_the_other_kind() {
        assert!(same_kind(
            Popout::Connection(Tab::Connection),
            Popout::Connection(Tab::Privacy)
        ));
        assert!(!same_kind(Popout::Noise, Popout::Connection(Tab::Privacy)));
    }
}
