//! The running activity's page: a `dc-webhost` child surface over the call view's stage, the Embedded App SDK bridge
//! behind it, and the answers to the commands that need the network or the user.
//!
//! Compositing (Appendix C spike 5): the surface paints above GPUI, so it is hidden whenever one of the app's overlays
//! (a menu, a modal, the shelf) is open, and when the call view is not the page on screen.

use super::activity_launch::ACTIVITY_PREFIX;
use super::tooltip::{Body, Side, Tip};
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
    let vw = f32::from(size.width);
    let vh = f32::from(size.height);
    let w = 320_f32.min((vw - 16.).max(160.));
    let h = 180_f32.min((vh - 16.).max(90.));
    (
        (vw - 400.).clamp(8., (vw - w - 8.).max(8.)),
        (vh - 312.).clamp(8., (vh - h - 8.).max(8.)),
        w,
        h,
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

/// Module 677793: secondary-bundle (`590467.js`) intl helper with English fallback when the key
/// is not present in the primary `177354.js` table.
pub(crate) fn pip_intl_or(key: &'static str, fallback: &'static str) -> String {
    if dc_intl::has_key(key) {
        dc_intl::t(key).to_owned()
    } else {
        fallback.to_owned()
    }
}

/// Module 677793 (`dPf03z` "Open {appName}"): tooltip for maximizing the Activity PiP back into stage.
pub(crate) fn pip_open_label(app_name: &str) -> String {
    if dc_intl::has_key("dPf03z") {
        dc_intl::format(
            "dPf03z",
            &[(
                "appName",
                dc_intl::Arg::Str(std::borrow::Cow::Borrowed(app_name)),
            )],
        )
    } else {
        format!("Open {app_name}")
    }
}

/// Module 677793: audio mute toggle label (`dxBzpe` "Turn off sound" / `bd0Ufh` "Turn on sound").
pub(crate) fn pip_mute_label(muted: bool) -> String {
    if muted {
        pip_intl_or("bd0Ufh", "Turn on sound")
    } else {
        pip_intl_or("dxBzpe", "Turn off sound")
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
        let invite_app = app.clone();
        let invite_name = name.clone();
        let invite_btn = self.with_tip(
            "activity-invite-tip",
            Tip::text(dc_intl::t("6F9ivu"), Side::Top),
            div()
                .id("activity-invite-play")
                .size(px(32.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|style| style.bg(color("--interactive-background-hover")))
                .child(icon("user_plus", 18., "--interactive-icon-default"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let is_private = this
                        .stores
                        .me()
                        .and_then(|u| this.stores.presences.get(&u.id))
                        .is_some_and(|s| s == "invisible");
                    this.open_activity_invite_modal(
                        invite_app.clone(),
                        invite_name.clone(),
                        format!("party-{}", invite_app),
                        is_private,
                        cx,
                    );
                })),
            cx,
        );
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
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(invite_btn)
                        .child(leave),
                )
                .into_any_element(),
        )
    }

    /// Module 677793 — Floating Activity / Conjure Frame PiP window (`card_SX_Cl2`, 320x180):
    /// renders the activity's cover art, a 40px top hover toolbar (`toolbar_SX_Cl2`, mini guild
    /// icon, app name, Open/Maximize, Hide overlay, and `VkMnR2` "Close App"), plus the
    /// bottom-right audio mute pill (`dxBzpe` / `bd0Ufh`). Drawn above the consent scrim.
    pub(super) fn activity_pip_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        // The video window takes the place first (`pipVideoWindow ?? pipActivityWindow`, module 197808), unless the
        // user switched to Activity via module 6155 (`PictureInPictureStore`).
        if !self.activities.pip || self.activities.pip_hidden || self.video_pip_shown() {
            return None;
        }
        let running = self.activities.running.as_ref()?;
        let (x, y, width, height) = pip_bounds(window);
        let app_id = running.application_id.clone();
        let raw_app_name = if running.name.is_empty() {
            self.activities
                .names
                .get(&app_id)
                .cloned()
                .unwrap_or_default()
        } else {
            running.name.clone()
        };
        let target_guild = running.guild;
        let target_channel = running.channel;
        let ch_name = self
            .stores
            .guild(target_guild)
            .and_then(|g| g.channels.iter().find(|c| c.id == target_channel))
            .map(|c| c.name.as_str());
        let resolved_title = pip_activity_title(
            (!raw_app_name.is_empty()).then_some(raw_app_name.as_str()),
            ch_name,
        );
        let app_name = if resolved_title.is_empty() {
            "Activity".to_owned()
        } else {
            resolved_title.to_owned()
        };
        let me_id = self.stores.current_user.as_ref().map(|u| u.id).unwrap_or(0);
        let pip_candidates: Vec<ActivityPipCandidate> = self
            .stores
            .voice_members(target_guild, target_channel)
            .iter()
            .map(|vs| ActivityPipCandidate {
                user_id: vs.user_id,
                is_activity: false,
                is_stream: vs.self_stream,
                has_video: vs.self_video || vs.self_stream,
            })
            .collect();
        let _featured_pip_participant = select_activity_pip_participant(&pip_candidates, me_id);
        let cover = self.images.get(&format!("shelf-banner:{app_id}")).cloned();
        let guild_icon_img = self.images.get(&format!("guild:{target_guild}")).cloned();
        let guild_acronym: String = self
            .stores
            .guild(target_guild)
            .map(|g| {
                g.name
                    .split_whitespace()
                    .filter_map(|w| w.chars().next())
                    .take(2)
                    .collect()
            })
            .unwrap_or_default();
        let guild_badge = (target_guild != 0).then(|| match guild_icon_img {
            Some(pic) => img(pic)
                .size(px(20.))
                .rounded(px(4.))
                .flex_none()
                .into_any_element(),
            None => div()
                .size(px(20.))
                .rounded(px(4.))
                .flex_none()
                .bg(color("--background-brand"))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(9.))
                .font_weight(FontWeight(600.))
                .text_color(color("--white"))
                .child(guild_acronym)
                .into_any_element(),
        });
        let muted = self.activities.pip_muted;
        let open_tip = pip_open_label(&app_name);
        let hide_tip = pip_intl_or("tfKtLw", "Hide overlay");
        let close_tip = dc_intl::t("VkMnR2");
        let mute_tip = pip_mute_label(muted);
        #[cfg(feature = "capture")]
        let controls_always_visible = self.activities.pip_controls_pinned;
        #[cfg(not(feature = "capture"))]
        let controls_always_visible = false;

        let toolbar_btn = |id: &'static str, glyph: &'static str, tint: &'static str| {
            div()
                .id(id)
                .size(px(26.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(color("--interactive-background-hover")))
                .child(icon(glyph, 16., tint))
        };

        let open_btn = self.with_tip(
            "activity-pip-open-tip",
            Tip::body(Body::Text(open_tip.into()), Side::Top),
            toolbar_btn(
                "activity-pip-open",
                "maximize",
                "--interactive-icon-default",
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.activities.pip = false;
                if target_guild != 0 {
                    this.guild = Some(target_guild);
                }
                if target_channel != 0 {
                    this.channel = Some(target_channel);
                }
                cx.notify();
            })),
            cx,
        );
        let hide_btn = self.with_tip(
            "activity-pip-hide-tip",
            Tip::body(Body::Text(hide_tip.into()), Side::Top),
            toolbar_btn(
                "activity-pip-hide",
                "minimize",
                "--interactive-icon-default",
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.activities.pip_hidden = true;
                cx.notify();
            })),
            cx,
        );
        let app_id_num = app_id.parse::<dc_state::Id>().unwrap_or_default();
        let more_btn = self.with_tip(
            "activity-pip-more-tip",
            Tip::text(dc_intl::t("SS9Y6P"), Side::Top),
            toolbar_btn(
                "activity-pip-more",
                "more_horizontal",
                "--interactive-icon-default",
            )
            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                if app_id_num != 0 {
                    this.open_context_menu(
                        super::context_menu::Target::ActivityPopoutOverflow {
                            app_id: app_id_num,
                            show_device_permissions: false,
                        },
                        event.position(),
                        cx,
                    );
                }
            })),
            cx,
        );
        let close_btn = self.with_tip(
            "activity-pip-close-tip",
            Tip::text(close_tip, Side::Top),
            toolbar_btn("activity-pip-close", "close", "--interactive-icon-default").on_click(
                cx.listener(|this, _, _, cx| {
                    this.leave_activity_by_hand(cx);
                    cx.notify();
                }),
            ),
            cx,
        );

        let toolbar = div()
            .parity("activity-pip-toolbar")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(40.))
            .px(px(10.))
            .bg(color("--background-surface-high"))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .when(!controls_always_visible, |bar| {
                bar.invisible()
                    .group_hover("activity-pip-card", |s| s.visible())
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .children(guild_badge)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(app_name),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(more_btn)
                    .child(open_btn)
                    .child(hide_btn)
                    .child(close_btn),
            );

        let mute_badge = div().absolute().bottom(px(8.)).right(px(8.)).child(
            self.with_tip(
                "activity-pip-mute-tip",
                Tip::body(Body::Text(mute_tip.into()), Side::Top),
                div()
                    .id("activity-pip-mute")
                    .size(px(28.))
                    .rounded_full()
                    .bg(gpui::rgba(0x111214cc))
                    .border_1()
                    .border_color(gpui::rgba(0xffffff22))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(gpui::rgba(0x111214ee)))
                    .child(icon(
                        if muted {
                            "clips_voice_off"
                        } else {
                            "clips_voice_on"
                        },
                        16.,
                        if muted { "--status-danger" } else { "--white" },
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.activities.pip_muted = !this.activities.pip_muted;
                        cx.notify();
                    })),
                cx,
            ),
        );

        let switcher = self.activity_pip_switcher_overlay(cx);
        Some(
            deferred(
                div()
                    .parity("activity-pip")
                    .id("activity-pip")
                    .group("activity-pip-card")
                    .occlude()
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(width))
                    .h(px(height))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .overflow_hidden()
                    .bg(color("--background-surface-high"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.32),
                        offset: gpui::point(px(0.), px(8.)),
                        blur_radius: px(20.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .children(
                        cover
                            .map(|cover| img(cover).size_full().object_fit(gpui::ObjectFit::Cover)),
                    )
                    .when(
                        self.images.get(&format!("shelf-banner:{app_id}")).is_none(),
                        |card| {
                            card.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .bg(gpui::rgba(0x1b1e2bff))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("activity_game", 44., "--text-muted")),
                            )
                        },
                    )
                    .child(toolbar)
                    .child(mute_badge)
                    .children(switcher),
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
            Ask::ShareLink {
                nonce,
                message,
                custom_id,
                link_id,
            } => {
                let (app_id, app_name) = self
                    .activities
                    .running
                    .as_ref()
                    .map(|r| (r.application_id.clone(), r.name.clone()))
                    .unwrap_or_else(|| ("0".to_owned(), "Activity".to_owned()));
                let me = self.stores.current_user.as_ref().map(|u| u.id.to_string());
                let share_url = super::forward::build_activity_share_url(
                    &app_id,
                    custom_id.as_deref(),
                    link_id.as_deref(),
                    me.as_deref(),
                );
                let content = if message.trim().is_empty() {
                    share_url.clone()
                } else {
                    format!("{message}\n{share_url}")
                };
                self.open_share_activity(
                    super::forward::ShareActivity {
                        application_id: app_id,
                        application_name: app_name,
                        preview_title: None,
                        subtitle: (!message.trim().is_empty()).then_some(message),
                        share_url,
                        content,
                        image_url: None,
                        media_counts: (0, 0, 0),
                        extra_media: 0,
                        sdk_cmd: Some(("SHARE_LINK".to_owned(), nonce)),
                        did_copy_link: false,
                    },
                    None,
                    cx,
                );
            }
            Ask::ShareContent { nonce, args } => {
                let (app_id, app_name) = self
                    .activities
                    .running
                    .as_ref()
                    .map(|r| (r.application_id.clone(), r.name.clone()))
                    .unwrap_or_else(|| ("0".to_owned(), "Activity".to_owned()));
                let me = self.stores.current_user.as_ref().map(|u| u.id.to_string());
                let custom_id = args["custom_id"].as_str();
                let link_id = args["link_id"].as_str();
                let share_url = super::forward::build_activity_share_url(
                    &app_id,
                    custom_id,
                    link_id,
                    me.as_deref(),
                );
                let preview_title = args["preview_title"]
                    .as_str()
                    .or_else(|| args["title"].as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);
                let subtitle = args["subtitle"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);
                let content = args["content"]
                    .as_str()
                    .or_else(|| args["message"].as_str())
                    .unwrap_or(&share_url)
                    .to_owned();
                let image_url = args["media_urls"]
                    .as_array()
                    .and_then(|arr| arr.first())
                    .and_then(Value::as_str)
                    .or_else(|| args["image_url"].as_str())
                    .map(str::to_owned);
                let mut imgs = 0usize;
                let mut vids = 0usize;
                let mut files = 0usize;
                if let Some(arr) = args["media_urls"].as_array() {
                    for u in arr.iter().filter_map(Value::as_str) {
                        let lower = u.to_lowercase();
                        if lower.ends_with(".mp4")
                            || lower.ends_with(".webm")
                            || lower.ends_with(".mov")
                        {
                            vids += 1;
                        } else if lower.ends_with(".png")
                            || lower.ends_with(".jpg")
                            || lower.ends_with(".jpeg")
                            || lower.ends_with(".gif")
                            || lower.ends_with(".webp")
                        {
                            imgs += 1;
                        } else {
                            files += 1;
                        }
                    }
                }
                let total_media = imgs + vids + files;
                self.open_share_activity(
                    super::forward::ShareActivity {
                        application_id: app_id,
                        application_name: app_name,
                        preview_title,
                        subtitle,
                        share_url,
                        content,
                        image_url,
                        media_counts: (imgs, vids, files),
                        extra_media: total_media.saturating_sub(1),
                        sdk_cmd: Some(("SHARE_CONTENT".to_owned(), nonce)),
                        did_copy_link: false,
                    },
                    None,
                    cx,
                );
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

    #[cfg(feature = "capture")]
    pub(crate) fn script_activity_pip(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.eq_ignore_ascii_case("close") {
            self.activities.pip = false;
            self.activities.pip_controls_pinned = false;
            self.activities.running = None;
            cx.notify();
            return "activity-pip: closed".to_owned();
        }
        let gid = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        let cid = self.channel.unwrap_or(1);
        self.activities.running = Some(super::activity_launch::Running {
            application_id: "880218394199220334".to_owned(),
            name: "Watch Together".to_owned(),
            launch_id: "capture-launch".to_owned(),
            location_id: "capture-loc".to_owned(),
            channel: cid,
            guild: gid,
            instance: super::activity_launch::Instance {
                application_id: "880218394199220334".to_owned(),
                launch_id: "capture-launch".to_owned(),
                composite_instance_id: "capture-comp".to_owned(),
                location_id: "capture-loc".to_owned(),
                channel_id: Some(cid.to_string()),
                guild_id: Some(gid.to_string()),
                participants: Vec::new(),
            },
            ticket: None,
            url: None,
            frame_id: "capture-frame".to_owned(),
            started: std::time::Instant::now(),
        });
        self.activities.pip = true;
        self.activities.pip_hidden = false;
        self.activities.pip_muted = arg.eq_ignore_ascii_case("muted");
        self.activities.pip_controls_pinned = true;
        cx.notify();
        format!("activity-pip: open muted={}", self.activities.pip_muted)
    }
}

/// Module 12830 (`111518.n`): 7 lifecycle states of the Voice Channel Conjure / Embedded App Surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum VoiceAppSurfaceState {
    Launched,
    RenderingElsewhere,
    NoApplication,
    DoesNotSupportSurface,
    Error,
    AwaitingLaunch,
    Loading,
}

/// Module 12830: candidate voice participant for the single-slot Activity PiP overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActivityPipCandidate {
    pub user_id: dc_state::Id,
    pub is_activity: bool,
    pub is_stream: bool,
    pub has_video: bool,
}

/// Module 12830 (`A`): selects the single priority non-self video participant for the PiP overlay
/// (excludes `ACTIVITY`, self, and non-video; prefers first `STREAM` participant else first video).
pub(crate) fn select_activity_pip_participant(
    candidates: &[ActivityPipCandidate],
    me_id: dc_state::Id,
) -> Option<dc_state::Id> {
    let eligible = candidates
        .iter()
        .filter(|c| !c.is_activity && c.user_id != me_id && c.has_video);
    eligible
        .clone()
        .find(|c| c.is_stream)
        .or_else(|| eligible.into_iter().next())
        .map(|c| c.user_id)
}

/// Module 12830 (`378859.A`): localized status heading & optional error line for each non-launched state.
pub(crate) fn voice_app_surface_status_copy(
    state: VoiceAppSurfaceState,
) -> Option<(String, Option<String>)> {
    match state {
        VoiceAppSurfaceState::RenderingElsewhere => Some((dc_intl::t("2KIDX+").to_owned(), None)),
        VoiceAppSurfaceState::NoApplication => Some((
            if dc_intl::has_key("TQptZV") {
                dc_intl::t("TQptZV").to_owned()
            } else {
                "This app couldn't be found.".to_owned()
            },
            None,
        )),
        VoiceAppSurfaceState::DoesNotSupportSurface => Some((
            if dc_intl::has_key("13FYwI") {
                dc_intl::t("13FYwI").to_owned()
            } else {
                "This app can't run in a voice channel.".to_owned()
            },
            None,
        )),
        VoiceAppSurfaceState::Error => Some((
            dc_intl::t("VquUff").to_owned(),
            Some(dc_intl::t("Sd9D/R").to_owned()),
        )),
        _ => None,
    }
}

/// Module 56129 (`Mp` / `gE` / `tM`): embedded-activity PiP control modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum EmbeddedActivityPipMode {
    /// `tM`: interactive PiP collapsible 12px -> 40px top header with `2TiKgS` / `oN8bqe` chevron.
    InteractiveHeader { expanded: bool, show_expand: bool },
    /// `Mp`: text-channel activity PiP overlay (top back/title + bottom participants + leave).
    TextActivity,
    /// `gE`: voice-channel non-interactive activity PiP overlay (video toggle + popout + leave).
    VoiceActivity {
        can_popout: bool,
        camera_unavailable: bool,
    },
}

/// Module 56129 (`F`): expand/collapse aria-label (`2TiKgS` "Collapse" vs `oN8bqe` "Picture in Picture, Expand").
pub(crate) fn pip_expand_aria_label(expanded: bool) -> &'static str {
    if expanded {
        dc_intl::t("2TiKgS")
    } else {
        dc_intl::t("oN8bqe")
    }
}

/// Module 56129: resolves PiP header title (`application.name ?? channel_name ?? ""`).
pub(crate) fn pip_activity_title<'a>(
    app_name: Option<&'a str>,
    channel_name: Option<&'a str>,
) -> &'a str {
    app_name
        .filter(|s| !s.trim().is_empty())
        .or_else(|| channel_name.filter(|s| !s.trim().is_empty()))
        .unwrap_or("")
}

impl Root {
    /// Module 12830 (`VoiceChannelAppSurface`, CSS `649914`): renders the 16:9 embedded app stage
    /// over `--black` with all 7 lifecycle states (`Launched`, `RenderingElsewhere`, `NoApplication`,
    /// `DoesNotSupportSurface`, `Error`, `AwaitingLaunch`/`Loading`).
    pub(super) fn render_voice_channel_app_surface(
        &self,
        state: VoiceAppSurfaceState,
    ) -> AnyElement {
        let stage_inner: AnyElement = match state {
            VoiceAppSurfaceState::Launched => div()
                .size_full()
                .child(self.anchor_probe("activity-stage"))
                .into_any_element(),
            VoiceAppSurfaceState::AwaitingLaunch | VoiceAppSurfaceState::Loading => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(super::voice_view::spinner())
                .into_any_element(),
            _ => {
                let (heading, err) = voice_app_surface_status_copy(state)
                    .unwrap_or_else(|| (dc_intl::t("VquUff").to_owned(), None));
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .p(px(24.))
                    .child(icon("home_shop", 32., "--icon-muted"))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child(heading),
                    )
                    .children(err.map(|e| {
                        div()
                            .text_size(px(13.))
                            .text_color(color("--text-feedback-critical"))
                            .child(e)
                    }))
                    .into_any_element()
            }
        };
        div()
            .absolute()
            .left(px(8.))
            .right(px(8.))
            .top(px(56.))
            .bottom(px(96.))
            .rounded(px(8.))
            .bg(color("--black"))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .child(div().size_full().child(stage_inner))
            .into_any_element()
    }

    /// Module 56129 (`tM` / `Mp` / `gE` / `ll`): renders the embedded-activity PiP controls bar.
    #[allow(dead_code)]
    pub(super) fn render_embedded_activity_pip_controls(
        &self,
        mode: EmbeddedActivityPipMode,
        title: &str,
        show_toggle_participants: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match mode {
            EmbeddedActivityPipMode::InteractiveHeader {
                expanded,
                show_expand,
            } => div()
                .w_full()
                .h(if expanded { px(40.) } else { px(12.) })
                .px(px(8.))
                .bg(color("--black"))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(if expanded {
                            title.to_owned()
                        } else {
                            String::new()
                        }),
                )
                .when(expanded, |row| {
                    row.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .when(show_expand, |btns| {
                                btns.child(
                                    div()
                                        .id("pip-activity-expand")
                                        .size(px(24.))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .aria_label(gpui::SharedString::from(
                                            pip_expand_aria_label(expanded),
                                        ))
                                        .child(icon("chevron_down", 16., "--white")),
                                )
                            })
                            .child(
                                div()
                                    .id("pip-activity-leave")
                                    .size(px(24.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .aria_label(gpui::SharedString::from(dc_intl::t("R/FK4A")))
                                    .child(icon("modal_close", 14., "--white"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.leave_activity(cx);
                                    })),
                            ),
                    )
                })
                .into_any_element(),
            EmbeddedActivityPipMode::TextActivity
            | EmbeddedActivityPipMode::VoiceActivity { .. } => div()
                .relative()
                .size_full()
                .p(px(8.))
                .flex()
                .flex_col()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(icon("clips_back", 16., "--white"))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(title.to_owned()),
                        ),
                )
                .when(show_toggle_participants, |box_el| {
                    box_el.child(
                        div()
                            .id("pip-activity-participants-toggle")
                            .absolute()
                            .right(px(16.))
                            .top(px(72.))
                            .size(px(28.))
                            .rounded_full()
                            .bg(gpui::rgba(0x00000099))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("members_nav", 14., "--white")),
                    )
                })
                .child(
                    div().flex().items_center().justify_end().child(
                        div()
                            .id("pip-activity-bottom-leave")
                            .size(px(28.))
                            .rounded_full()
                            .bg(color("--control-critical-primary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .aria_label(gpui::SharedString::from(dc_intl::t("R/FK4A")))
                            .child(icon("modal_close", 14., "--white"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.leave_activity(cx);
                            })),
                    ),
                )
                .into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActivityPipCandidate, VoiceAppSurfaceState, elapsed_text, pip_activity_title,
        pip_expand_aria_label, pip_mute_label, pip_open_label, select_activity_pip_participant,
        voice_app_surface_status_copy,
    };

    #[test]
    fn the_counter_reads_like_the_official_one() {
        assert_eq!(elapsed_text(3), "00:03 elapsed");
        assert_eq!(elapsed_text(75), "01:15 elapsed");
        assert_eq!(elapsed_text(3600 + 61), "01:01:01 elapsed");
    }

    #[test]
    fn pip_toolbar_labels_match_module_677793() {
        assert!(pip_open_label("Watch Together").contains("Watch Together"));
        assert_eq!(pip_mute_label(false), "Turn off sound");
        assert_eq!(pip_mute_label(true), "Turn on sound");
        assert_eq!(dc_intl::t("VkMnR2"), "Close App");
    }

    #[test]
    fn voice_app_surface_12830_and_pip_controls_56129() {
        for key in ["2KIDX+", "VquUff", "Sd9D/R", "2TiKgS", "oN8bqe", "R/FK4A"] {
            assert!(dc_intl::has_key(key), "missing 12830/56129 key {key}");
        }
        let cands = vec![
            ActivityPipCandidate {
                user_id: 1,
                is_activity: false,
                is_stream: true,
                has_video: true,
            },
            ActivityPipCandidate {
                user_id: 2,
                is_activity: false,
                is_stream: false,
                has_video: true,
            },
            ActivityPipCandidate {
                user_id: 3,
                is_activity: false,
                is_stream: true,
                has_video: true,
            },
        ];
        // Self (1) excluded; stream (3) preferred over non-stream video (2).
        assert_eq!(select_activity_pip_participant(&cands, 1), Some(3));
        assert!(voice_app_surface_status_copy(VoiceAppSurfaceState::RenderingElsewhere).is_some());
        assert!(voice_app_surface_status_copy(VoiceAppSurfaceState::Error).is_some());
        assert_eq!(pip_expand_aria_label(true), dc_intl::t("2TiKgS"));
        assert_eq!(pip_expand_aria_label(false), dc_intl::t("oN8bqe"));
        assert_eq!(pip_activity_title(Some("Chess"), Some("General")), "Chess");
        assert_eq!(pip_activity_title(None, Some("General")), "General");
    }
}
