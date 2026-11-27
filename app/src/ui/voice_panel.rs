//! The voice panel's popouts over its status row: the connection popout (module 926641, the `RTCConnection` stats and
//! Upload Logs, and the Privacy tab with the call's code) and the noise suppression popout (`TL` in module 596339).
//! Measured in `design/dom/popouts/voice-panel-ping-darker-cozy.json` and `voice-panel-krisp-darker-cozy.json`.
//!
//! The official noise popout names Krisp. gravecord does not run Krisp: the same popout names what runs,
//! DeepFilterNet.

mod ping_graph;
pub(crate) mod rtc_debug;

use super::motion::{PopKind, PopSide, Popout as _};
use super::{Root, color, icon, tooltip};
use dc_state::Id;
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, IntoElement, Window, deferred, div,
    linear_color_stop, linear_gradient, prelude::*, px,
};
use ping_graph::Sample;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TROUBLESHOOTING: &str = "https://support.discord.com/hc/en-us/articles/360045138471";
/// `getArticleURL(VOICE_CONNECTION_ERRORS)` (article `115001310031`, module 652215).
const CONNECTION_ERRORS: &str = "https://support.discord.com/hc/en-us/articles/115001310031";
/// `qF7.STATUS` of module 818348.
const STATUS_PAGE: &str = "https://discordstatus.com";
const NOISE_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/360040843952";
const ENCRYPTION_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/25968222946071";
const DEEP_FILTER_NET: &str = "https://github.com/Rikorose/DeepFilterNet";
const DEEP_FILTER_NET_NAME: &str = "DeepFilterNet";
const PRIVACY_CODE_FONT: &str = "Consolas";

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
    /// `_pings`: `{time: Date.now(), value}` per ping, oldest first.
    pings: Vec<Sample>,
    /// A stand-in for `RTCConnection.hostname` when no voice server is known (harness captures only).
    hostname: Option<String>,
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
    /// `_handlePing`, stamped with the wall clock as `Date.now()`.
    pub fn ping(&mut self, ms: f64) {
        self.ping_at(now_ms(), ms);
    }

    /// `_handlePing` at a given time, ms since the epoch.
    pub fn ping_at(&mut self, time: f64, ms: f64) {
        self.pings.push(Sample { time, value: ms });
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

    #[cfg(test)]
    pub fn set_hostname(&mut self, hostname: String) {
        self.hostname = Some(hostname);
    }

    #[cfg(test)]
    pub(super) fn privacy_code(&self) -> Option<&str> {
        self.privacy_code.as_deref()
    }

    /// `getAveragePing`: of the first twenty kept, as the official slice reads.
    pub fn average(&self) -> f64 {
        let first = &self.pings[..self.pings.len().min(PINGS_AVERAGED)];
        if first.is_empty() {
            return 0.;
        }
        first.iter().map(|ping| ping.value).sum::<f64>() / first.len() as f64
    }

    pub fn last(&self) -> Option<f64> {
        self.pings.last().map(|ping| ping.value)
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
    /// The icon of module 382404 (`D`): three arcs when fine, two when average, one when bad, and the arcs with a
    /// warning sign while nothing is known.
    pub fn glyph(self) -> &'static str {
        match self {
            Quality::Fine => "voice_ping",
            Quality::Average => "voice_ping_average",
            Quality::Bad => "voice_ping_bad",
            Quality::Unknown => "voice_ping_unknown",
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

/// `Date.now()`.
fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0., |since| since.as_secs_f64() * 1000.)
}

/// `getShortHostname` of `RTCConnection.hostname`: the endpoint's host up to its first dot.
fn short_hostname(endpoint: &str) -> &str {
    let host = endpoint.rsplit("//").next().unwrap_or(endpoint);
    let host = host.split([':', '/']).next().unwrap_or(host);
    host.split('.').next().unwrap_or(host)
}

/// The tab panel's height: both panels share one grid cell (`tabPanel_TP1WMf > * {grid-area: 1/1}`), so it is the
/// taller of the two. The Privacy panel without a code is 251 (`voice-panel-ping-darker-cozy.json`); the Copy button
/// and its gap add 40. The Connection panel with the graph adds the graph block, its 16px margin, the 18px host name
/// line and one 8px gap; its paragraph wraps to five lines in the measured popout.
fn panel_height(code_known: bool, graph: bool) -> f32 {
    let privacy = if code_known { 291. } else { 251. };
    // Stats 48, the paragraph 80, the rule 1, the buttons 32, the banner 24 and five 8px gaps.
    let connection = 217. + if graph { GRAPH_BLOCK } else { 0. };
    f32::max(privacy, connection)
}

const GRAPH_BLOCK: f32 = ping_graph::CONTAINER_HEIGHT + 16. + 18. + 8.;

/// Splits an intl message around its `$link` run so any locale preserves link placement.
fn split_link_message(key: &str, values: &dc_intl::Args<'_>) -> (String, String, String) {
    let mut before = String::new();
    let mut link = String::new();
    let mut after = String::new();
    let mut seen_link = false;
    for (part, tag) in dc_intl::format_runs(key, values) {
        if !seen_link && tag.as_deref() == Some("$link") {
            link.push_str(&part);
            seen_link = true;
        } else if seen_link {
            after.push_str(&part);
        } else {
            before.push_str(&part);
        }
    }
    (before, link, after)
}

/// `getStatus` of module 917592: the panel's status line for a state, as a string key and the colour of its class
/// (`rtcConnectionStatusConnected` positive, `...Connecting` warning, `...Error` critical, module 587669's CSS).
pub(crate) fn rtc_status(state: dc_voice::RtcState, video: bool) -> (&'static str, &'static str) {
    use dc_voice::RtcState::*;
    const CONNECTING: &str = "--text-feedback-warning";
    const CONNECTED: &str = "--text-feedback-positive";
    const ERROR: &str = "--text-feedback-critical";
    match state {
        Connecting => ("MzW9sN", CONNECTING),
        Authenticating => ("GxXwE2", CONNECTING),
        AwaitingEndpoint => ("uQle7a", CONNECTING),
        RtcConnected => (if video { "HtVOdd" } else { "daXg45" }, CONNECTED),
        RtcConnecting => ("Gp51dl", CONNECTING),
        IceChecking => ("rdCyA/", CONNECTING),
        DtlsConnecting => ("UvB3gV", CONNECTING),
        NoRoute => ("mGhOIi", ERROR),
        RtcDisconnected => ("M7LDmE", ERROR),
        Disconnected => ("NLKQbx", ERROR),
    }
}

/// The connection popout's text for a state that is not `RTC_CONNECTED` (`w` of module 926641), with the link it
/// carries, if any; `None` when connected, where the numbers show instead.
pub(crate) fn rtc_explanation(
    state: dc_voice::RtcState,
) -> Option<(&'static str, Option<&'static str>)> {
    use dc_voice::RtcState::*;
    Some(match state {
        AwaitingEndpoint => ("Eu2vUR", Some(STATUS_PAGE)),
        Connecting => ("y+E8aD", None),
        Authenticating => ("5lGIZH", None),
        Disconnected | RtcDisconnected => ("fOX25I", None),
        RtcConnecting => ("b5Ubd5", None),
        IceChecking | DtlsConnecting => ("SyoYUb", Some(CONNECTION_ERRORS)),
        NoRoute => ("2tgQnk", Some(CONNECTION_ERRORS)),
        RtcConnected => return None,
    })
}

#[cfg(test)]
mod rtc_state_tests {
    use super::{rtc_explanation, rtc_status};
    use dc_voice::RtcState::*;

    #[test]
    fn each_state_has_its_title_colour_and_explanation_from_modules_917592_and_926641() {
        assert_eq!(
            rtc_status(RtcConnected, false),
            ("daXg45", "--text-feedback-positive")
        );
        assert_eq!(rtc_status(RtcConnected, true).0, "HtVOdd");
        assert_eq!(
            rtc_status(AwaitingEndpoint, false),
            ("uQle7a", "--text-feedback-warning")
        );
        assert_eq!(
            rtc_status(NoRoute, false),
            ("mGhOIi", "--text-feedback-critical")
        );
        assert_eq!(rtc_status(Disconnected, false).0, "NLKQbx");
        for state in [
            Disconnected,
            AwaitingEndpoint,
            Authenticating,
            Connecting,
            RtcDisconnected,
            RtcConnecting,
            NoRoute,
            IceChecking,
            DtlsConnecting,
        ] {
            assert!(rtc_explanation(state).is_some(), "{state:?}");
        }
        assert_eq!(rtc_explanation(RtcConnected), None);
        assert_eq!(
            rtc_explanation(NoRoute),
            Some((
                "2tgQnk",
                Some("https://support.discord.com/hc/en-us/articles/115001310031")
            ))
        );
        assert_eq!(
            rtc_explanation(AwaitingEndpoint),
            Some(("Eu2vUR", Some("https://discordstatus.com")))
        );
        assert_eq!(rtc_explanation(RtcDisconnected), Some(("fOX25I", None)));
    }
}

/// The troubleshooting line under the numbers (`3pFz1P`, or `vggaMt` while no loss is known), split around its link.
fn advice(loss_known: bool) -> (String, String, String) {
    if loss_known {
        split_link_message(
            "3pFz1P",
            &[
                ("badPing", dc_intl::Arg::Num(f64::from(BAD_PING))),
                ("badLossRate", dc_intl::Arg::Num(f64::from(BAD_LOSS))),
                ("url", TROUBLESHOOTING.into()),
            ],
        )
    } else {
        split_link_message(
            "vggaMt",
            &[
                ("badPing", dc_intl::Arg::Num(f64::from(BAD_PING))),
                ("url", TROUBLESHOOTING.into()),
            ],
        )
    }
}

/// Port of modules `926641` (`h.k`), `259374` (`s`), and `83942` (`!c.c()`):
/// `(show_tab_bar, show_privacy_tab)`.
pub(crate) fn connection_tab_flags(
    connected: bool,
    is_stage: bool,
    dave_version: u16,
    has_others: bool,
) -> (bool, bool) {
    let encrypted = connected && !is_stage && dave_version != 0;
    (encrypted, encrypted && has_others)
}

/// Shared 32px secondary button styling for `[Debug]`, `[Upload Logs]`, `[Copy]`, and mic `[Test]`.
fn secondary_button(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
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
    Soundboard,
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
    /// The graph's repaint timer runs (module 229659 repaints on animation frames while the canvas is shown).
    graph_ticking: bool,
    /// Pointer X coordinate relative to the `240px` ping plot box while hovering.
    pub ping_hover_x: Option<f32>,
}

fn same_kind(a: Popout, b: Popout) -> bool {
    matches!(
        (a, b),
        (Popout::Connection(_), Popout::Connection(_))
            | (Popout::Noise, Popout::Noise)
            | (Popout::Soundboard, Popout::Soundboard)
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

    pub(super) fn close_panel_popout(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = self.panel.popout.take() {
            self.panel.closed_at = Some((open, Instant::now()));
            self.panel.ping_hover_x = None;
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
        if self.panel.popout == Some(Popout::Soundboard) && self.soundboard_disabled().is_some() {
            self.close_panel_popout(cx);
        }
        let body = match self.panel.popout? {
            Popout::Connection(tab) => self.connection_popout(tab, window, cx),
            Popout::Noise => self.noise_popout(window, cx),
            Popout::Soundboard => return self.soundboard_picker(window, cx),
        };
        Some(deferred(body).with_priority(3).into_any_element())
    }

    /// `popover_81IzEA` at the ping, 300 wide.
    fn connection_popout(
        &mut self,
        tab: Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let call = self.call.as_ref();
        let connected = call.is_some_and(|call| call.connected);
        let rtc_state = call.map_or(dc_voice::RtcState::Disconnected, |call| {
            if call.connected {
                dc_voice::RtcState::RtcConnected
            } else {
                call.rtc_state
            }
        });
        let stats = call.map(|call| call.stats.clone()).unwrap_or_default();
        let others = call.is_some_and(|call| {
            call.has_others(self.stores.current_user.as_ref().map(|user| user.id))
        });
        let is_stage =
            call.is_some_and(|call| self.is_stage_voice_channel(call.guild, call.channel));
        let dave_version = call.map_or(0, |call| call.dave_version);
        // `RTCConnection.hostname`, from the voice server the call was handed.
        let hostname = call
            .and_then(|call| {
                self.stores.voice_server.as_ref().filter(|server| {
                    server.guild_id == call.guild
                        && (call.guild != 0 || server.channel_id == Some(call.channel))
                })
            })
            .map(|server| short_hostname(&server.endpoint).to_owned())
            .or_else(|| stats.hostname.clone())
            .unwrap_or_default();
        let (encrypted, show_privacy_tab) =
            connection_tab_flags(connected, is_stage, dave_version, others);
        let tab = if show_privacy_tab {
            tab
        } else {
            if tab != Tab::Connection {
                self.panel.popout = Some(Popout::Connection(Tab::Connection));
            }
            Tab::Connection
        };
        // Module 926641 draws the graph and the host name only with Developer Mode on (`developerMode`, module
        // 253932) while connected.
        let developer = self.developer_mode();
        if developer && connected && tab == Tab::Connection {
            self.tick_ping_graph(window.scale_factor(), cx);
        }
        let code_known = stats.privacy_code.is_some();
        let panel = if encrypted {
            panel_height(code_known, developer)
        } else {
            0.
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
        let connection_tab = tab_item(
            "voice-tab-connection",
            dc_intl::t!("MBY1Pm"),
            Tab::Connection,
        );
        let privacy_tab = tab_item("voice-tab-privacy", dc_intl::t!("zC6o3s"), Tab::Privacy);
        let separator = || div().h(px(1.)).bg(color("--border-subtle"));
        // `securedBanner_ymaupl card_qCN-Ri cardSuccess`: 24 tall, padding 4px 8px, radius 8, pushed to the bottom
        // (`container > :last-child {margin-top: auto}`).
        let banner = |text: &'static str| {
            div()
                .mt_auto()
                .h(px(24.))
                .px(px(8.))
                .rounded(px(8.))
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
                let stat = |key: &'static str, var: &'static str, value: String| {
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-default"))
                        .child(super::bold_message(
                            key,
                            &[(var, value.into())],
                            FontWeight(600.),
                        ))
                };
                let (before, link, after) = advice(stats.loss.is_some());
                // Any state but `RTC_CONNECTED`: one `body-sm/normal` `text-muted` paragraph in place of the numbers.
                let explanation = rtc_explanation(rtc_state).map(|(key, url)| {
                    let paragraph = div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"));
                    match url {
                        Some(url) => {
                            let (before, link, after) =
                                split_link_message(key, &[("url", url.into())]);
                            paragraph
                                .child(self.text_with_link(
                                    "voice-panel-state",
                                    &before,
                                    &link,
                                    &after,
                                    cx,
                                    move |_, cx| cx.open_url(url),
                                ))
                                .into_any_element()
                        }
                        None => paragraph.child(dc_intl::t(key)).into_any_element(),
                    }
                });
                let numbers = explanation.is_none();
                let uploading = self.panel.uploading;
                let done = self.panel.upload_done_at.is_some();
                // `graphContainer_BlzJTq` and the host name (`body-sm/semibold`, `text-default`).
                let graph = (developer && connected).then(|| {
                    let frame = ping_graph::Frame::at(
                        &stats.pings,
                        now_ms(),
                        f64::from(window.scale_factor()),
                    )
                    .with_hover(self.panel.ping_hover_x);
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id("voice-panel-ping-graph")
                                .relative()
                                .w(px(ping_graph::WIDTH))
                                .h(px(ping_graph::CONTAINER_HEIGHT))
                                .mb(px(16.))
                                .bg(color("--background-base-lower"))
                                .child(self.anchor_probe("voice-panel-ping-graph"))
                                .on_mouse_move(cx.listener(
                                    |this, event: &gpui::MouseMoveEvent, _, cx| {
                                        let Some(bounds) = this
                                            .tip_anchors
                                            .borrow()
                                            .get("voice-panel-ping-graph")
                                            .copied()
                                        else {
                                            return;
                                        };
                                        let local_x = f32::from(event.position.x - bounds.origin.x);
                                        let next = (0. ..=ping_graph::PLOT_WIDTH)
                                            .contains(&local_x)
                                            .then_some(local_x);
                                        if this.panel.ping_hover_x != next {
                                            this.panel.ping_hover_x = next;
                                            cx.notify();
                                        }
                                    },
                                ))
                                .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                                    if !*inside && this.panel.ping_hover_x.take().is_some() {
                                        cx.notify();
                                    }
                                }))
                                .child(ping_graph::graph(frame)),
                        )
                        .child(
                            div()
                                .h(px(18.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-default"))
                                .child(hostname.clone()),
                        )
                });
                div()
                    .min_h(px(panel))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(graph)
                    .children(explanation)
                    .when(numbers, |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .child(stat(
                                    "X58/lN",
                                    "averagePing",
                                    format!("{:.0}", stats.average()),
                                ))
                                .children(
                                    stats.last().map(|last| {
                                        stat("6iv2TF", "lastPing", format!("{last:.0}"))
                                    }),
                                )
                                .children(stats.loss.map(|loss| {
                                    stat("VIBJM+", "outboundLossRate", format!("{loss:.1}"))
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
                                    &link,
                                    &after,
                                    cx,
                                    |_, cx| cx.open_url(TROUBLESHOOTING),
                                )),
                        )
                    })
                    .child(separator())
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .when(developer, |this| {
                                this.child(
                                    secondary_button("voice-panel-debug")
                                        .flex_1()
                                        .cursor_pointer()
                                        .hover(|style| {
                                            style.bg(color("--control-secondary-background-hover"))
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_panel_popout(cx);
                                            this.open_rtc_debug(None, cx);
                                        }))
                                        .child(icon(
                                            "voice_panel_debug",
                                            16.,
                                            "--control-secondary-text-default",
                                        ))
                                        .child(dc_intl::t!("KBoWg9")),
                                )
                            })
                            .child(
                                secondary_button("voice-panel-upload")
                                    .flex_1()
                                    .when(uploading, |this| this.opacity(0.6))
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
                                    // `success` swaps the upload icon for `CircleCheckIcon`.
                                    .when(!uploading, |this| {
                                        this.child(icon(
                                            if done {
                                                "circle_check"
                                            } else {
                                                "voice_panel_upload_logs"
                                            },
                                            16.,
                                            "--control-secondary-text-default",
                                        ))
                                    })
                                    .child(if done {
                                        dc_intl::t!("i4jeWR")
                                    } else {
                                        dc_intl::t!("EbwFfR")
                                    }),
                            ),
                    )
                    .child(banner(if encrypted {
                        dc_intl::t!("3BogKe")
                    } else {
                        dc_intl::t!("ETIVvg")
                    }))
                    .into_any_element()
            }
            Tab::Privacy => {
                let code = stats.privacy_code.clone().unwrap_or_default();
                let chunks = code_chunks(&code);
                let copied = self.panel.copied_at.is_some();
                let (e2ee_before, e2ee_link, e2ee_after) =
                    split_link_message("wKxADe", &[("helpArticle", ENCRYPTION_ARTICLE.into())]);
                let code_row = |slice: &[String]| {
                    div()
                        .flex()
                        .gap(px(8.))
                        .children(slice.iter().cloned().map(|chunk| {
                            div()
                                .flex_1()
                                .text_center()
                                .text_size(px(14.))
                                .line_height(px(22.))
                                .font_weight(FontWeight(500.))
                                .font_family(PRIVACY_CODE_FONT)
                                .text_color(color("--text-default"))
                                .child(chunk)
                        }))
                };
                div()
                    .min_h(px(panel))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t!("cTQI5t")),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(self.text_with_link(
                                "voice-panel-e2ee",
                                &e2ee_before,
                                &e2ee_link,
                                &e2ee_after,
                                cx,
                                |_, cx| cx.open_url(ENCRYPTION_ARTICLE),
                            )),
                    )
                    .child(
                        div()
                            .min_h(px(104.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lower"))
                            .when(code_known, |this| {
                                let first = &chunks[..chunks.len().min(3)];
                                let rest = chunks.get(3..).unwrap_or_default();
                                this.p(px(12.))
                                    .flex()
                                    .flex_col()
                                    .justify_center()
                                    .gap(px(16.))
                                    .child(code_row(first))
                                    .when(!rest.is_empty(), |col| {
                                        col.child(separator()).child(code_row(rest))
                                    })
                            })
                            // While the epoch has no authenticator the grid holds the 32px wandering cubes at its
                            // start (`loading_PpKUqH` in the dump).
                            .when(!code_known, |this| {
                                this.p(px(12.))
                                    .flex()
                                    .items_center()
                                    .child(super::login::wandering_cubes())
                            }),
                    )
                    // The Copy button exists only once there is a code (`null != s`).
                    .when(code_known, |this| {
                        this.child(
                            secondary_button("voice-panel-copy")
                                .cursor_pointer()
                                .hover(|style| {
                                    style.bg(color("--control-secondary-background-hover"))
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let spaced = code_chunks(&code).join(" ");
                                    cx.write_to_clipboard(ClipboardItem::new_string(spaced));
                                    this.panel.copied_at = Some(Instant::now());
                                    cx.spawn(async move |this, cx| {
                                        cx.background_executor()
                                            .timer(Duration::from_secs(2))
                                            .await;
                                        let _ = this.update(cx, |this, cx| {
                                            this.panel.copied_at = None;
                                            cx.notify();
                                        });
                                    })
                                    .detach();
                                    cx.notify();
                                }))
                                .child(icon(
                                    if copied {
                                        "circle_check"
                                    } else {
                                        "menu_copy_text"
                                    },
                                    16.,
                                    "--control-secondary-text-default",
                                ))
                                .child(if copied {
                                    dc_intl::t!("t5VZ88")
                                } else {
                                    dc_intl::t!("OpuAlK")
                                }),
                        )
                    })
                    .child(separator())
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-subtle"))
                            .child(dc_intl::t!("B9JNsl")),
                    )
                    .child(banner(dc_intl::t!("3BogKe")))
                    .into_any_element()
            }
        };
        div()
            .id("voice-connection-popout")
            .popout("voice-connection-popout", PopKind::Translate, PopSide::Top)
            .absolute()
            .left(px(20.))
            .bottom(px(POPOUT_BOTTOM))
            .w(px(310.))
            .p(px(15.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .shadow(vec![super::shadow_high()])
            .occlude()
            .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                if this.press_outside(super::keyboard::Layer::VoicePanelPopout, cx)
                    && event.button != gpui::MouseButton::Right
                {
                    cx.stop_propagation();
                }
            }))
            .flex()
            .flex_col()
            .gap(px(16.))
            .when(encrypted, |this| {
                this.child(
                    div()
                        .flex()
                        .gap(px(24.))
                        .border_b_1()
                        .border_color(color("--border-subtle"))
                        .child(connection_tab)
                        .when(show_privacy_tab, |bar| bar.child(privacy_tab)),
                )
            })
            .child(content)
            .into_any_element()
    }

    /// Module 229659 repaints on every animation frame while the canvas is shown; the picture only moves when the
    /// window's end passes a whole device pixel, so the popout redraws at those moments.
    fn tick_ping_graph(&mut self, scale_factor: f32, cx: &mut Context<Self>) {
        if self.panel.graph_ticking {
            return;
        }
        self.panel.graph_ticking = true;
        let step = 1000. / f64::from(scale_factor.max(0.5));
        cx.spawn(async move |this, cx| {
            loop {
                let wait = step - now_ms().rem_euclid(step);
                cx.background_executor()
                    .timer(Duration::from_secs_f64(wait.max(1.) / 1000.))
                    .await;
                let shown = this
                    .update(cx, |this, cx| {
                        let shown = this.panel.popout == Some(Popout::Connection(Tab::Connection))
                            && this.developer_mode();
                        if shown {
                            cx.notify();
                        } else {
                            this.panel.graph_ticking = false;
                        }
                        shown
                    })
                    .unwrap_or(false);
                if !shown {
                    break;
                }
            }
        })
        .detach();
    }

    /// Harness only: the connection popout with a ping history (`pinggraph open | privacy | peer | solo | code |
    /// rtcdebug | clear | points N | screenshot | long | host <endpoint> | state`). Nothing is sent.
    #[cfg(feature = "capture")]
    pub(crate) fn script_ping_history(&mut self, args: &[&str], cx: &mut Context<Self>) -> String {
        match args {
            ["open"] => {
                self.panel.popout = Some(Popout::Connection(Tab::Connection));
                cx.notify();
                return "the connection popout is open".to_owned();
            }
            ["open", "privacy"] | ["privacy"] => {
                self.panel.popout = Some(Popout::Connection(Tab::Privacy));
                cx.notify();
                return "the privacy tab is open".to_owned();
            }
            ["hover-ping", "clear"] => {
                self.panel.ping_hover_x = None;
                cx.notify();
                return "ping hover cleared".to_owned();
            }
            ["hover-ping", frac] => {
                let Ok(frac) = frac.parse::<f32>() else {
                    return format!("cannot read {frac}");
                };
                let x = frac.clamp(0., 1.) * ping_graph::PLOT_WIDTH;
                self.panel.ping_hover_x = Some(x);
                cx.notify();
                return format!("ping hover x={x:.1}");
            }
            ["rtcdebug", "open"] => {
                self.close_panel_popout(cx);
                self.open_rtc_debug(None, cx);
                return "rtc debug window opened".to_owned();
            }
            ["rtcdebug", "close"] => {
                self.close_rtc_debug(cx);
                return "rtc debug window closed".to_owned();
            }
            ["rtcdebug", "section", section] => {
                self.close_panel_popout(cx);
                self.open_rtc_debug(Some((*section).to_owned()), cx);
                return format!("rtc debug opened at {section}");
            }
            ["rtcdebug", "localip"] => {
                let mut data = rtc_debug::build_live_data(self, cx);
                if let Some(transport) = data
                    .default_stats
                    .first_mut()
                    .and_then(|conn| conn.transport.as_mut())
                {
                    transport.local_address = Some("192.168.1.44:50000".to_owned());
                }
                rtc_debug::set_debug_stats_override(Some(data), cx);
                return "rtc debug local_address override installed".to_owned();
            }
            ["rtcdebug", "streamer", state] => {
                let on = matches!(*state, "on" | "1" | "true");
                let user = self.stores.current_user.as_ref().map(|user| user.id);
                crate::notifications::set_switch("streaming_enable_streamer_mode", on, user);
                crate::notifications::set_switch(
                    "streamer_mode_hide_personal_information",
                    on,
                    user,
                );
                cx.notify();
                return format!("rtc debug streamer mode: {on}");
            }
            _ => {}
        }
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let Some(call) = self.call.as_mut() else {
            return "no call: run dmcall fake-join first".to_owned();
        };
        let now = now_ms();
        let line = match args {
            ["clear"] => {
                call.stats.pings.clear();
                "no pings".to_owned()
            }
            ["points", count] => {
                let Ok(count) = count.parse::<usize>() else {
                    return format!("cannot read {count}");
                };
                call.stats.pings.clear();
                for n in 0..count {
                    call.stats.ping_at(
                        now - 5_000. * (count - n) as f64,
                        98. + (n % 3) as f64 * 3.,
                    );
                }
                format!("{} pings", call.stats.pings.len())
            }
            // Shaped like a screenshot of the official popout: about 100 ms every five seconds for four and a half
            // minutes, with spikes to 500-700 ms.
            ["screenshot"] => {
                call.stats.pings.clear();
                const SPIKES: [(usize, f64); 7] = [
                    (14, 230.),
                    (17, 520.),
                    (19, 700.),
                    (30, 260.),
                    (33, 240.),
                    (40, 560.),
                    (47, 250.),
                ];
                for n in 0..54usize {
                    let base = 96. + ((n * 7) % 11) as f64;
                    let value = SPIKES
                        .iter()
                        .find(|(at, _)| *at == n)
                        .map_or(base, |(_, value)| *value);
                    call.stats.ping_at(now - 5_000. * (54 - n) as f64, value);
                }
                call.stats.set_loss(0.);
                format!("{} pings", call.stats.pings.len())
            }
            // More than the 199 kept, one a second.
            ["long"] => {
                call.stats.pings.clear();
                for n in 0..400usize {
                    let value = if n % 37 == 0 {
                        420.
                    } else {
                        60. + (n % 9) as f64 * 4.
                    };
                    call.stats.ping_at(now - 1_000. * (400 - n) as f64, value);
                }
                format!("{} pings kept", call.stats.pings.len())
            }
            ["host", endpoint] => {
                call.stats.hostname = Some(short_hostname(endpoint).to_owned());
                format!("host {}", short_hostname(endpoint))
            }
            ["peer"] | ["peer", _] => {
                let peer = args
                    .get(1)
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(1_570_000_000_000_000_100);
                let mut users = call.rtc_users().to_vec();
                if let Some(me) = me
                    && !users.contains(&me)
                {
                    users.push(me);
                }
                if !users.contains(&peer) {
                    users.push(peer);
                }
                call.set_rtc_users(users);
                call.dave_version = call.dave_version.max(1);
                format!("peer {peer} present ({} users)", call.rtc_users().len())
            }
            ["solo"] => {
                call.set_rtc_users(me.into_iter().collect());
                "solo voice channel".to_owned()
            }
            ["dave", version] => {
                let Ok(version) = version.parse::<u16>() else {
                    return format!("cannot read {version}");
                };
                call.dave_version = version;
                format!("dave_version {version}")
            }
            ["code", code] => {
                let value = if *code == "sample" {
                    "123456789012345678901234567890"
                } else {
                    *code
                };
                call.stats.set_privacy_code(value.to_owned());
                format!("privacy code set ({} chars)", value.len())
            }
            ["state"] => {
                let frame = ping_graph::Frame::at(&call.stats.pings, now, 1.);
                format!(
                    "{} pings, average {:.0}, last {:?}, labels {:?}, ticks {}",
                    call.stats.pings.len(),
                    call.stats.average(),
                    call.stats.last(),
                    frame.axis.labels,
                    frame.ticks.len()
                )
            }
            _ => "usage: pinggraph open | privacy | hover-ping <frac|clear> | peer | solo | dave N | code <30|sample> | rtcdebug open|close|section|localip | clear | points N | screenshot | long | host <endpoint> | state"
                .to_owned(),
        };
        cx.notify();
        line
    }

    /// `noiseCancellationPopout_9U6LUN`: 276 wide, centred over the button.
    fn noise_popout(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let description = dc_intl::t!("uKdWn6").replace("Krisp", DEEP_FILTER_NET_NAME);
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
                        match super::plugin_hooks::noise_popout_off().filter(|_| on) {
                            // Not in the pinned client: AltKrispSwitch turns it off to None
                            // (`plugin_hooks/alt_krisp_switch.rs`).
                            Some(index) => this.set_noise_suppression_index(index),
                            None => this.set_noise_cancellation(!on),
                        }
                        cx.notify();
                    }))
            });
        let switch: AnyElement = match self.voice_profile_label().filter(|_| locked) {
            Some(profile) => {
                let input_profile_id = if profile == "Studio" {
                    "STUDIO"
                } else {
                    "VOICE_ISOLATION"
                };
                let tip_text = dc_intl::format(
                    "/tqmfM",
                    &[
                        ("enabled", if on { "true" } else { "false" }.into()),
                        ("inputProfile", input_profile_id.into()),
                    ],
                );
                self.with_tip(
                    "voice-noise-switch-tip",
                    tooltip::Tip::body(tooltip::Body::Text(tip_text.into()), tooltip::Side::Top),
                    switch,
                    cx,
                )
                .into_any_element()
            }
            None => switch.into_any_element(),
        };
        let test_button = self.small_mic_test(window, cx);
        div()
            .id("voice-noise-popout")
            .popout("voice-noise-popout", PopKind::Translate, PopSide::Top)
            .absolute()
            .left(px(164.))
            .bottom(px(POPOUT_BOTTOM))
            .w(px(NOISE_WIDTH))
            .p(px(15.))
            .rounded(px(4.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .shadow(vec![super::shadow_high()])
            .occlude()
            .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                if this.press_outside(super::keyboard::Layer::VoicePanelPopout, cx)
                    && event.button != gpui::MouseButton::Right
                {
                    cx.stop_propagation();
                }
            }))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label(dc_intl::t!("WGWHv/")))
                    .child(switch),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(description),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(label(dc_intl::t!("JdUas1")))
                    .child(test_button),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(label(dc_intl::t!("k6h1F4")))
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
                                    .child(DEEP_FILTER_NET_NAME),
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
                                    .child(dc_intl::t!("hvVgAZ")),
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
        let button = secondary_button("voice-noise-test")
            .w(px(60.))
            .flex_none()
            .cursor_pointer()
            .hover(|style| style.bg(color("--control-secondary-background-hover")))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_mic_test(cx)))
            .child(if testing {
                dc_intl::t!("Yp3SbJ")
            } else {
                dc_intl::t!("sG+MGg")
            });
        let button: AnyElement =
            if self.call.as_ref().is_some_and(|call| call.connected) && !testing {
                self.with_tip(
                    "voice-noise-test-tip",
                    tooltip::Tip::body(
                        tooltip::Body::Text(dc_intl::t!("9viE2A").into()),
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
        if guild == 0 {
            // A DM call's second line opens the DM.
            self.select_home(Some(channel));
        } else {
            self.open_voice_view(guild, channel, cx);
        }
    }

    /// Share Your Screen, panel and bar: the Go Live picker; while our stream runs the click stops it (`Tv` of module
    /// 596339).
    pub(super) fn voice_screen_clicked(&mut self, cx: &mut Context<Self>) {
        if self.own_stream().is_some() {
            self.stream_stop(cx);
        } else {
            self.open_go_live(cx);
        }
    }

    /// Start An Activity, panel and bar: the shelf.
    pub(super) fn voice_activity_clicked(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.open_activity_shelf(window, cx);
    }
}

/// Module 773262 (`StreamPrivacyCodeModal`): displays a Go Live stream's 30-digit DAVE MLS epoch
/// authenticator (6 chunks of 5 digits, 3 columns x 2 rows) or loading spinner.
#[derive(Clone, Debug)]
pub(crate) struct StreamPrivacyCodeModal {
    pub owner: dc_state::Id,
    /// Optional demo/override code for capture & tests when no live stream DAVE epoch is active.
    pub demo_code: Option<String>,
    pub copied: bool,
}

impl Root {
    /// Opens module 773262 (`openSecureFramesStreamVerification`) for `owner`'s Go Live stream.
    pub(super) fn open_stream_privacy_code(&mut self, owner: dc_state::Id, cx: &mut Context<Self>) {
        self.stream_privacy_code_modal = Some(StreamPrivacyCodeModal {
            owner,
            demo_code: None,
            copied: false,
        });
        cx.notify();
    }

    /// Module 773262 (`75811.j` + `686418` `.verification_jKs4FP`): Stream Privacy Code modal layer.
    pub(super) fn stream_privacy_code_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.stream_privacy_code_modal.as_ref()?;
        let live_code = self
            .stream_privacy_code_of(modal.owner)
            .or_else(|| modal.demo_code.clone());
        let chunks = live_code
            .as_deref()
            .map(code_chunks)
            .filter(|c| !c.is_empty());
        let copied = modal.copied;
        let copy_value = chunks.as_ref().map(|c| c.join(" "));
        let (foot_before, foot_link, foot_after) = if dc_intl::has_key("H3+ktv") {
            split_link_message("H3+ktv", &[("helpArticle", ENCRYPTION_ARTICLE.into())])
        } else {
            (
                "A new code is generated when people join or leave this stream. ".to_owned(),
                "Learn more about end-to-end encryption.".to_owned(),
                String::new(),
            )
        };

        let code_row = |slice: &[String]| {
            div()
                .flex()
                .gap(px(8.))
                .children(slice.iter().cloned().map(|chunk| {
                    div()
                        .flex_1()
                        .text_center()
                        .text_size(px(14.))
                        .line_height(px(22.))
                        .font_weight(FontWeight(700.))
                        .font_family(PRIVACY_CODE_FONT)
                        .text_color(color("--text-default"))
                        .child(chunk)
                }))
        };

        let grid_body: AnyElement = if let Some(chunks) = chunks {
            let first = &chunks[..chunks.len().min(3)];
            let rest = chunks.get(3..).unwrap_or_default();
            div()
                .min_h(px(104.))
                .p(px(12.))
                .rounded_b(px(12.))
                .border_1()
                .border_t_0()
                .border_color(color("--border-muted"))
                .bg(color("--background-base-lower"))
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(16.))
                .child(code_row(first))
                .when(!rest.is_empty(), |col| {
                    col.child(div().h(px(1.)).bg(color("--border-subtle")))
                        .child(code_row(rest))
                })
                .into_any_element()
        } else {
            div()
                .min_h(px(104.))
                .p(px(12.))
                .rounded_b(px(12.))
                .border_1()
                .border_t_0()
                .border_color(color("--border-muted"))
                .bg(color("--background-base-lower"))
                .flex()
                .items_center()
                .justify_center()
                .child(super::login::wandering_cubes())
                .into_any_element()
        };

        let card = div()
            .id("stream-privacy-code-modal")
            .occlude()
            .w(px(440_f32.min((self.viewport.0 - 48.).max(320.))))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![super::shadow_high()])
            .overflow_hidden()
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("QogHld")),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("qODBkW")),
                            ),
                    )
                    .child(
                        div()
                            .id("stream-privacy-code-close")
                            .size(px(32.))
                            .flex_none()
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--control-icon-only-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 16., "--control-icon-only-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.stream_privacy_code_modal = None;
                                cx.notify();
                            })),
                    ),
            )
            // `.verification_jKs4FP`: `.header_jKs4FP` + `.code_jKs4FP`.
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(16.))
                            .py(px(8.))
                            .rounded_t(px(12.))
                            .border_1()
                            .border_color(color("--border-muted"))
                            .bg(color("--background-mod-normal"))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("cgBTyO")),
                            )
                            .when_some(copy_value, |bar, text| {
                                bar.child(
                                    div()
                                        .id("stream-privacy-code-copy")
                                        .px(px(6.))
                                        .py(px(2.))
                                        .rounded(px(4.))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .child(icon(
                                            if copied { "circle_check" } else { "clips_copy" },
                                            14.,
                                            if copied {
                                                "--text-feedback-positive"
                                            } else {
                                                "--interactive-text-default"
                                            },
                                        ))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(color(if copied {
                                                    "--text-feedback-positive"
                                                } else {
                                                    "--text-muted"
                                                }))
                                                .child(dc_intl::t(if copied {
                                                    "t5VZ88"
                                                } else {
                                                    "OpuAlK"
                                                })),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                text.clone(),
                                            ));
                                            if let Some(m) = this.stream_privacy_code_modal.as_mut()
                                            {
                                                m.copied = true;
                                            }
                                            cx.notify();
                                        })),
                                )
                            }),
                    )
                    .child(grid_body),
            )
            // `.footer_jKs4FP`: centered `H3+ktv` with E2EE help article link.
            .child(
                div()
                    .w_full()
                    .text_center()
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child(self.text_with_link(
                        "stream-privacy-e2ee",
                        &foot_before,
                        &foot_link,
                        &foot_after,
                        cx,
                        |_, cx| cx.open_url(ENCRYPTION_ARTICLE),
                    )),
            );

        Some(
            deferred(
                div()
                    .id("stream-privacy-code-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(color("--background-scrim"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.stream_privacy_code_modal = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(95)
            .into_any_element(),
        )
    }

    /// Capture harness: `stream-privacy-code [loading|close]` (module 773262).
    #[cfg(feature = "capture")]
    pub(crate) fn script_stream_privacy_code(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.eq_ignore_ascii_case("close") {
            self.stream_privacy_code_modal = None;
            cx.notify();
            return "stream-privacy-code: closed".to_owned();
        }
        let demo_code = if arg.eq_ignore_ascii_case("loading") {
            None
        } else {
            Some("482910374651928374650192837465".to_owned())
        };
        self.stream_privacy_code_modal = Some(StreamPrivacyCodeModal {
            owner: 1,
            demo_code,
            copied: false,
        });
        cx.notify();
        format!("stream-privacy-code: open ({arg})")
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
    fn pings_keep_their_times_and_the_oldest_leave_first() {
        let mut stats = CallStats::default();
        for n in 0..250 {
            stats.ping_at(f64::from(n) * 1000., f64::from(n));
        }
        // `while (_pings.length >= 200) shift()`: 199 stay, the newest ones, in order.
        assert_eq!(stats.pings.len(), 199);
        assert_eq!(
            stats.pings[0],
            Sample {
                time: 51_000.,
                value: 51.
            }
        );
        assert_eq!(stats.last(), Some(249.));
        assert!(
            stats
                .pings
                .windows(2)
                .all(|pair| pair[0].time < pair[1].time)
        );
        // A live ping is stamped with the wall clock.
        let before = now_ms();
        stats.ping(80.);
        let stamped = stats.pings.last().map(|ping| ping.time).unwrap_or_default();
        assert!(stamped >= before && stamped <= now_ms());
    }

    #[test]
    fn the_host_name_is_the_endpoint_up_to_its_first_dot() {
        assert_eq!(
            short_hostname("c-fra14-db667712.discord.media:443"),
            "c-fra14-db667712"
        );
        assert_eq!(
            short_hostname("wss://c-ams01-abcd.discord.media"),
            "c-ams01-abcd"
        );
        assert_eq!(short_hostname("plain:80"), "plain");
        assert_eq!(short_hostname(""), "");
    }

    #[test]
    fn the_tab_panel_is_as_tall_as_its_taller_panel() {
        assert_eq!(panel_height(false, false), 251.);
        assert_eq!(panel_height(true, false), 291.);
        // 217 of the Connection panel plus the 85 graph block, its 16 margin, the 18 host name and one 8 gap.
        assert_eq!(panel_height(false, true), 344.);
        assert_eq!(panel_height(true, true), 344.);
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

    #[test]
    fn encrypted_and_alone_gate_connection_tabs_and_privacy_tab() {
        // Connected non-stage DAVE call with peers shows both tabs.
        assert_eq!(connection_tab_flags(true, false, 1, true), (true, true));
        // Encrypted solo call shows the Connection tab bar without the Privacy tab.
        assert_eq!(connection_tab_flags(true, false, 1, false), (true, false));
        // Stage channels, DAVE version 0, and disconnected calls hide the entire tab bar.
        assert_eq!(connection_tab_flags(true, true, 1, true), (false, false));
        assert_eq!(connection_tab_flags(true, false, 0, true), (false, false));
        assert_eq!(connection_tab_flags(false, false, 1, true), (false, false));
    }

    #[test]
    fn intl_link_splits_preserve_link_label_for_advice_and_e2ee() {
        let (before, link, after) =
            split_link_message("wKxADe", &[("helpArticle", ENCRYPTION_ARTICLE.into())]);
        assert!(before.contains("people join or leave"));
        assert_eq!(link, "Learn more about end-to-end encryption.");
        assert!(after.is_empty());

        let (adv_before, adv_link, adv_after) = advice(true);
        assert!(adv_before.contains("250 ms"));
        assert_eq!(adv_link, "troubleshooting guide");
        assert_eq!(adv_after, " for more assistance.");
    }

    #[test]
    fn call_stats_store_hostname_and_privacy_code() {
        let mut stats = CallStats::default();
        assert_eq!(stats.privacy_code(), None);
        stats.set_hostname("c-fra14-db667712".to_owned());
        stats.set_privacy_code("123456789012345678901234567890".to_owned());
        assert_eq!(stats.hostname.as_deref(), Some("c-fra14-db667712"));
        assert_eq!(stats.privacy_code(), Some("123456789012345678901234567890"));
        assert_eq!(
            code_chunks(stats.privacy_code().unwrap_or_default()).len(),
            6
        );
    }

    #[test]
    fn stream_privacy_code_773262_keys_and_chunks() {
        let chunks = code_chunks("482910374651928374650192837465");
        assert_eq!(chunks.len(), 6);
        assert_eq!(chunks[0], "48291");
        assert_eq!(chunks[5], "37465");
        for key in ["tjbJbf", "QogHld", "qODBkW", "cgBTyO", "OpuAlK", "t5VZ88"] {
            assert!(!dc_intl::t(key).is_empty(), "missing 773262 key {key}");
        }
    }
}
