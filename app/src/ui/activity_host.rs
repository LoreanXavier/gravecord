//! The running activity's page: a `dc-webhost` child surface over the call view's stage, the Embedded App SDK bridge
//! behind it, and the answers to the commands that need the network or the user.
//!
//! Compositing (Appendix C spike 5): the surface paints above GPUI, so it is hidden whenever one of the app's overlays
//! (a menu, a modal, the shelf) is open, and when the call view is not the page on screen.

use super::activity_launch::ACTIVITY_PREFIX;
use super::tooltip::{Side, Tip};
use super::{Root, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use dc_webhost::sdk::{self, Action, Ask, Bridge};
use dc_webhost::{Area, HostEvent, Placement, WebHost};
use gpui::{AnyElement, Context, FontWeight, Task, Window, deferred, div, img, prelude::*, px};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(40);

pub(crate) struct ActivityHost {
    web: WebHost,
    bridge: Bridge,
    ctx: sdk::Context,
    url: String,
    launch_id: String,
    ready: bool,
    visible: bool,
    bounds: Placement,
    /// Commands waiting for an answer: nonce to (command, the token the page handed over).
    pending: HashMap<String, (String, Option<String>)>,
    /// The commands the page sent, for the harness.
    seen: Vec<String>,
    _poll: Task<()>,
}

/// The picture-in-picture window (`pictureInPictureWindow_69M0s9`): 320 by 180, measured at 400 from the right edge and
/// 312 from the bottom of the 1280 by 720 frame.
fn pip_bounds(window: &Window) -> (f32, f32, f32, f32) {
    let size = window.viewport_size();
    (
        f32::from(size.width) - 400.,
        f32::from(size.height) - 312.,
        320.,
        180.,
    )
}

fn user_json(user: &dc_state::User) -> Value {
    let mut value = json!({
        "id": user.id.to_string(),
        "username": user.username,
        "discriminator": user.discriminator,
        "global_name": user.global_name,
        "avatar_decoration_data": null,
        "bot": user.bot,
        "flags": 0,
        "public_flags": 0,
        "premium_type": 0,
    });
    // For a page on `postMessage` a missing avatar is left out.
    if let Some(avatar) = &user.avatar {
        value["avatar"] = json!(avatar);
    }
    value
}

impl ActivityHost {
    /// For the capture harness: created, relay ready, handshake done, authenticated, commands seen.
    #[cfg(feature = "capture")]
    pub(super) fn state(&self) -> String {
        format!(
            "ready={} handshake={} authed={} visible={} commands={}",
            self.ready,
            self.bridge.origin().is_some(),
            self.bridge.authenticated(),
            self.visible,
            self.seen.join(",")
        )
    }
}

/// `renderTime` of the elapsed counter (module 492684): hours only when there are some, every part two digits.
pub(super) fn elapsed_text(seconds: u64) -> String {
    let (hours, minutes, seconds) = ((seconds / 3600) % 24, (seconds / 60) % 60, seconds % 60);
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02} elapsed")
    } else {
        format!("{minutes:02}:{seconds:02} elapsed")
    }
}

impl Root {
    /// `panel_8eCwJY activityPanel`: the visible game first, otherwise the embedded activity and Leave. It sits
    /// above the call panel (golden: 57px, a 12px pad, the 32px row, a 1px rule).
    pub(super) fn activity_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(panel) = self.running_game_panel(cx) {
            return Some(panel);
        }
        let running = self.activities.running.as_ref()?;
        let app = running.application_id.clone();
        let name = if running.name.is_empty() {
            self.activities.names.get(&app).cloned().unwrap_or_default()
        } else {
            running.name.clone()
        };
        let picture = self.images.get(&format!("shelf-icon:{app}")).cloned();
        let elapsed = elapsed_text(running.started.elapsed().as_secs());
        let leave = self.with_tip(
            "activity-leave-tip",
            Tip::text("Leave Activity", Side::Top),
            div()
                .id("activity-leave")
                .size(px(32.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|style| style.bg(color("--interactive-background-hover")))
                .child(icon("activity_leave", 20., "--interactive-icon-default"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.leave_activity_by_hand(cx);
                    cx.notify();
                })),
            cx,
        );
        Some(
            div()
                .parity_below("activity-panel", 1.)
                .id("activity-panel")
                .w_full()
                .h(px(57.))
                .flex_none()
                .px(px(12.))
                .py(px(12.))
                .bg(color("--background-surface-high"))
                .border_b_1()
                .border_color(color("--background-mod-subtle"))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .size(px(32.))
                                .flex_none()
                                .rounded(px(8.))
                                .overflow_hidden()
                                .children(
                                    picture
                                        .map(|picture| img(picture).size(px(32.)).rounded(px(8.))),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(13.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-subtle"))
                                        .child(elapsed),
                                ),
                        ),
                )
                .child(leave)
                .into_any_element(),
        )
    }

    /// What shows in the picture-in-picture window until the page paints over it: the application's cover on the
    /// card's own ground. Drawn above the consent scrim, like the window.
    pub(super) fn activity_pip_layer(&self, window: &Window) -> Option<AnyElement> {
        // The video window takes the place first (`pipVideoWindow ?? pipActivityWindow`, module 197808).
        if !self.activities.pip || self.video_pip_shown() {
            return None;
        }
        let running = self.activities.running.as_ref()?;
        let (x, y, width, height) = pip_bounds(window);
        let cover = self
            .images
            .get(&format!("shelf-banner:{}", running.application_id))
            .cloned();
        Some(
            deferred(
                div()
                    .parity("activity-pip")
                    .id("activity-pip")
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(width))
                    .h(px(height))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(color("--background-surface-high"))
                    .children(
                        cover
                            .map(|cover| img(cover).size_full().object_fit(gpui::ObjectFit::Cover)),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    fn activity_context(&self) -> Option<sdk::Context> {
        let running = self.activities.running.as_ref()?;
        let me = self.stores.current_user.as_ref()?;
        let participants = running
            .instance
            .participants
            .iter()
            .filter_map(|p| p.user_id.parse::<u64>().ok())
            .filter_map(|id| {
                self.stores
                    .users
                    .get(&id)
                    .cloned()
                    .or_else(|| Some(me.clone()).filter(|me| me.id == id))
            })
            .map(|user| user_json(&user))
            .collect();
        Some(sdk::Context {
            application_id: running.application_id.clone(),
            origin: super::activity_launch::activity_origin(&running.application_id),
            frame_id: running.frame_id.clone(),
            user: user_json(me),
            channel_id: running.instance.channel_id.clone(),
            guild_id: running.instance.guild_id.clone(),
            participants,
            locale: "en-US".to_owned(),
            cdn_host: "cdn.discordapp.com".to_owned(),
            api_endpoint: "//discord.com/api".to_owned(),
            custom_id: None,
            referrer_id: None,
        })
    }

    /// Whether one of the app's own overlays is up (the surface would cover it). The picture-in-picture window sits
    /// above the consent modal, as in the official frame, so that one does not hide it.
    fn overlay_open(&self, pip: bool) -> bool {
        self.vv.menu.is_some()
            || self.activity_shelf.is_some()
            || self.activities.dialog_open()
            || (self.oauth_open() && !pip)
            || self.voice_dialog_open()
            || self.quick_switcher.is_some()
    }

    /// The call view's stage in window coordinates, as the last frame painted it.
    fn activity_stage(&self) -> Option<(f32, f32, f32, f32)> {
        let bounds = self.tip_anchors.borrow().get("activity-stage").copied()?;
        Some((
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        ))
    }

    /// Makes the page match `activities.running`: created once its URL is known and the stage has been painted,
    /// placed on the stage, hidden under overlays, dropped when the activity ends. Called every frame.
    pub(super) fn activity_host_frame(&mut self, window: &Window, cx: &mut Context<Self>) {
        let wanted = self
            .activities
            .running
            .as_ref()
            .and_then(|running| Some((running.launch_id.clone(), running.url.clone()?)));
        let stage_seen = std::mem::take(&mut self.activities.stage_seen);
        self.activities.pip = self.activities.running.is_some() && !stage_seen;
        let Some((launch_id, url)) = wanted else {
            if self.activities.host.take().is_some() {
                cx.notify();
            }
            return;
        };
        if self
            .activities
            .host
            .as_ref()
            .is_some_and(|host| host.launch_id != launch_id || host.url != url)
        {
            self.activities.host = None;
        }
        // With the call view's stage out of sight the page is the picture-in-picture window.
        let pip = !stage_seen;
        // On the stage it also steps aside for any menu, popout or dialog of the app over it (`web_frames.rs`).
        let visible = !self.overlay_open(pip) && (pip || !self.web_surfaces_covered());
        let stage = if pip {
            Some(pip_bounds(window))
        } else {
            self.activity_stage()
        }
        .map(|bounds| Placement::whole(Area::logical(bounds, window.scale_factor())));
        if self.activities.host.is_none() {
            let (Some(bounds), Some(ctx)) = (stage, self.activity_context()) else {
                return;
            };
            let Some(dir) = crate::storage::data_dir().map(|dir| dir.join("webview")) else {
                return;
            };
            match WebHost::new(window, dir, bounds) {
                Ok(web) => {
                    web.set_visible(visible);
                    let task = cx.spawn(async move |this, cx| {
                        loop {
                            cx.background_executor().timer(POLL).await;
                            if this
                                .update(cx, |root, cx| root.activity_host_poll(cx))
                                .is_err()
                            {
                                return;
                            }
                        }
                    });
                    self.activities.host = Some(ActivityHost {
                        web,
                        bridge: Bridge::new(),
                        ctx,
                        url,
                        launch_id,
                        ready: false,
                        visible,
                        bounds,
                        pending: HashMap::new(),
                        seen: Vec::new(),
                        _poll: task,
                    });
                    tracing::info!("activity page created");
                }
                Err(error) => {
                    tracing::warn!("activity page could not be created: {error}");
                    self.activities.host_failed(&error.to_string());
                }
            }
            return;
        }
        let ctx = self.activity_context();
        let Some(host) = self.activities.host.as_mut() else {
            return;
        };
        if let Some(ctx) = ctx {
            let changed = ctx.participants != host.ctx.participants;
            host.ctx.participants = ctx.participants.clone();
            if changed {
                let actions = host.bridge.participants_changed(&host.ctx);
                self.activity_actions(actions, cx);
            }
        }
        let Some(host) = self.activities.host.as_mut() else {
            return;
        };
        if let Some(bounds) = stage.filter(|bounds| *bounds != host.bounds) {
            host.web.set_placement(bounds);
            host.bounds = bounds;
        }
        if visible != host.visible {
            host.web.set_visible(visible);
            host.visible = visible;
        }
        let actions = host.bridge.set_pip(pip);
        self.activity_actions(actions, cx);
    }

    /// The relay page's events since the last poll.
    fn activity_host_poll(&mut self, cx: &mut Context<Self>) {
        // The panel's counter shows whole seconds.
        let seconds = self
            .activities
            .running
            .as_ref()
            .map(|running| running.started.elapsed().as_secs());
        if seconds != self.activities.shown_seconds {
            self.activities.shown_seconds = seconds;
            cx.notify();
        }
        let Some(host) = self.activities.host.as_mut() else {
            return;
        };
        let events = host.web.poll();
        let mut actions: Vec<Action> = Vec::new();
        let mut failure: Option<String> = None;
        for event in events {
            host.seen.push(
                match &event {
                    HostEvent::Ready => "#ready",
                    HostEvent::Loaded => "#loaded",
                    HostEvent::Message { .. } => "#message",
                    HostEvent::Failed(_) => "#failed",
                }
                .to_owned(),
            );
            match event {
                HostEvent::Failed(why) => {
                    tracing::warn!("activity page failed: {why}");
                    failure = Some(why);
                }
                HostEvent::Ready => {
                    host.ready = true;
                    host.web.open(&host.url);
                }
                HostEvent::Loaded => {
                    // The frame's query parameters, as the page gets them in HELLO.
                    let query: serde_json::Map<String, Value> = host
                        .url
                        .split_once('?')
                        .map(|(_, query)| query)
                        .unwrap_or_default()
                        .split('&')
                        .filter_map(|pair| pair.split_once('='))
                        .filter(|(name, _)| *name != "discord_proxy_ticket")
                        .map(|(name, value)| (name.to_owned(), json!(value)))
                        .collect();
                    let hello = Bridge::hello(&host.ctx, &Value::Object(query));
                    let origin = host.ctx.origin.clone();
                    host.web.post(&hello, &origin);
                }
                HostEvent::Message { origin, data } => {
                    if let Some(cmd) = data[1]["cmd"].as_str().or(data[0]
                        .as_u64()
                        .map(|op| if op == 0 { "HANDSHAKE" } else { "" }))
                        && !cmd.is_empty()
                        && host.seen.len() < 40
                    {
                        host.seen.push(cmd.to_owned());
                    }
                    actions.extend(host.bridge.handle(&origin, &data, &host.ctx));
                }
            }
        }
        if let Some(why) = failure {
            self.activities.host = None;
            self.activities.host_failed(&why);
            self.leave_activity(cx);
            return;
        }
        self.activity_actions(actions, cx);
    }

    /// Carries out what the bridge decided.
    fn activity_actions(&mut self, actions: Vec<Action>, cx: &mut Context<Self>) {
        let mut disconnect = false;
        for action in actions {
            let Some(host) = self.activities.host.as_mut() else {
                return;
            };
            let origin = host
                .bridge
                .origin()
                .map(str::to_owned)
                .unwrap_or_else(|| host.ctx.origin.clone());
            match action {
                Action::Post(message) => host.web.post(&message, &origin),
                Action::Close { code, message } => {
                    host.web.post(
                        &json!([2, {"code": code, "message": message}]),
                        &host.ctx.origin,
                    );
                }
                Action::Unhandled(command) => {
                    tracing::debug!(
                        command,
                        "activity asked for something this client does not do"
                    );
                }
                Action::Disconnected { code } => {
                    tracing::info!(code, "activity closed its connection");
                    disconnect = true;
                }
                Action::Ask(ask) => self.activity_ask(ask, cx),
            }
        }
        if disconnect {
            self.leave_activity(cx);
        }
    }

    fn activity_ask(&mut self, ask: Ask, cx: &mut Context<Self>) {
        let Some(host) = self.activities.host.as_mut() else {
            return;
        };
        match ask {
            Ask::Authorize { nonce, args } => self.oauth_begin(nonce, &args, cx),
            Ask::Authenticate { nonce, token } => {
                host.pending.insert(
                    nonce.clone(),
                    ("AUTHENTICATE".to_owned(), Some(token.clone())),
                );
                let _ = self.requests.send(Request::BearerGet {
                    key: format!("{ACTIVITY_PREFIX}ask:{nonce}"),
                    route: "/oauth2/@me".to_owned(),
                    bearer: token,
                });
            }
            Ask::ProxyTicket { nonce } => {
                host.pending.insert(
                    nonce.clone(),
                    ("REQUEST_PROXY_TICKET_REFRESH".to_owned(), None),
                );
                let (Some(app), Some(channel)) = (
                    self.activities
                        .running
                        .as_ref()
                        .map(|r| r.application_id.clone()),
                    self.activities.running.as_ref().map(|r| r.channel),
                ) else {
                    return;
                };
                let _ = self.requests.send(Request::Rest {
                    key: Some(format!("{ACTIVITY_PREFIX}ask:{nonce}")),
                    method: reqwest::Method::POST,
                    route: format!("/applications/{app}/proxy-tickets"),
                    body: Some(super::activity_launch::ticket_body(channel)),
                });
            }
            Ask::EncourageHardwareAcceleration { nonce } => {
                let answer = self.encourage_hardware_acceleration();
                cx.notify();
                if let Some(host) = self.activities.host.as_mut() {
                    let action =
                        host.bridge
                            .answer("ENCOURAGE_HW_ACCELERATION", &nonce, Ok(answer));
                    self.activity_actions(vec![action], cx);
                }
            }
            Ask::OpenExternalLink { nonce, url } => {
                // The untrusted-link confirmation of the official client is not built: no link opens (BLOCKED.md).
                tracing::debug!(
                    length = url.len(),
                    "activity asked to open a link: not opened"
                );
                let action =
                    host.bridge
                        .answer("OPEN_EXTERNAL_LINK", &nonce, Ok(json!({"opened": false})));
                self.activity_actions(vec![action], cx);
            }
        }
    }

    /// Answers a command of the page that waited for the user or the network.
    pub(super) fn activity_reply(
        &mut self,
        cmd: &str,
        nonce: &str,
        result: Result<Value, (u32, String)>,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.activities.host.as_mut() else {
            return;
        };
        let action = host.bridge.answer(cmd, nonce, result);
        self.activity_actions(vec![action], cx);
    }

    /// The network answer to an `Ask`.
    pub(super) fn activity_ask_answered(
        &mut self,
        nonce: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let me = self
            .stores
            .current_user
            .as_ref()
            .map(|user| user.id.to_string());
        let app = self
            .activities
            .running
            .as_ref()
            .map(|r| r.application_id.clone());
        let Some(host) = self.activities.host.as_mut() else {
            return;
        };
        let Some((cmd, token)) = host.pending.remove(nonce) else {
            return;
        };
        let ok = body["ok"].as_bool() == Some(true);
        let data = &body["body"];
        let result: Result<Value, (u32, String)> = match cmd.as_str() {
            "AUTHENTICATE" => {
                if !ok {
                    Err((sdk::code::INVALID_TOKEN, "Invalid access token".to_owned()))
                } else if data["application"]["id"].as_str() != app.as_deref() {
                    Err((
                        sdk::code::INVALID_CLIENTID,
                        "Application does not match the connection's".to_owned(),
                    ))
                } else if data["user"]["id"].as_str() != me.as_deref() {
                    Err((
                        sdk::code::INVALID_TOKEN,
                        "Token does not match current user".to_owned(),
                    ))
                } else {
                    Ok(json!({
                        "application": data["application"],
                        "user": data["user"],
                        "access_token": token,
                        "expires": data["expires"],
                        "scopes": data["scopes"],
                    }))
                }
            }
            _ => match data["ticket"].as_str() {
                Some(ticket) if ok => {
                    if let Some(running) = self.activities.running.as_mut() {
                        running.ticket = Some(ticket.to_owned());
                    }
                    Ok(json!({"ticket": ticket}))
                }
                _ => Err((
                    sdk::code::UNKNOWN_ERROR,
                    "Failed to create proxy ticket".to_owned(),
                )),
            },
        };
        let Some(host) = self.activities.host.as_mut() else {
            return;
        };
        let action = host.bridge.answer(&cmd, nonce, result);
        self.activity_actions(vec![action], cx);
    }
}

#[cfg(test)]
mod tests {
    use super::elapsed_text;

    #[test]
    fn the_counter_reads_like_the_official_one() {
        assert_eq!(elapsed_text(3), "00:03 elapsed");
        assert_eq!(elapsed_text(75), "01:15 elapsed");
        assert_eq!(elapsed_text(3600 + 61), "01:01:01 elapsed");
    }
}
