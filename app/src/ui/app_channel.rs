//! App Channel full-pane view (`AppChannel`, module 98492, CSS `717519` / `728846`).
//!
//! Renders `GUILD_APP` (channel type `21`) and `DM_OPENS_ACTIVITY` embedded app channels:
//! - Missing app configuration (`tU5fiM` / `E94mJf`)
//! - User-closed relaunch card (`PPYvuv` "Launch App" + 64x64 rounded app icon)
//! - Active frame launcher lifecycle (`AwaitingLaunch`, `Loading`, `Launched`,
//!   `RenderingElsewhere` `2KIDX+`, `NoApplication` `izggZO`, `DoesNotSupportSurface` `iUWcU/`,
//!   `Error` `VquUff` + `Sd9D/R`)
//! - Update-available reload bar (`144399.x`) and floating 304px chat toasts column (`20465.A`)
//!   with auto-open chat sidebar guard on unread.

use super::{Root, color, icon};
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use std::collections::HashSet;

/// Module 98492 (`H`): embedded App Channel frame lifecycle states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AppChannelFramePhase {
    #[default]
    AwaitingLaunch,
    Loading,
    Launched,
    RenderingElsewhere,
    NoApplication,
    DoesNotSupportSurface,
    Error,
}

/// Module 98492: a compact chat toast displayed over a launched App Channel when the chat sidebar
/// is collapsed (`.chatToasts_lw58SW` / `.column_lw58SW` width `304px`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppChannelChatToast {
    pub message_id: Id,
    pub author: String,
    pub preview: String,
}

/// Module 98492 + 241606: per-client App Channel state (remembering user-closed channels in
/// `closed_channels` and Voice Channel App Surface toggle state in `voice_app_visible`).
#[derive(Clone, Debug, Default)]
pub(crate) struct AppChannelState {
    /// Module-level `Set` `O`: channels whose app frame the user explicitly closed (shows relaunch card).
    pub closed_channels: HashSet<Id>,
    /// Module 241606 (`VoiceChannelAppStore`): voice channels currently showing the embedded Channel App surface.
    pub voice_app_visible: HashSet<Id>,
    /// Capture/preview override modal or inline state.
    pub preview: Option<AppChannelPreview>,
}

/// Module 241606 (`fNq-Kh`): localized tooltip/aria label for the Voice Channel App View toggle button.
pub(crate) fn voice_channel_app_toggle_label(
    is_app_visible: bool,
    returns_to_room: bool,
) -> &'static str {
    if is_app_visible && returns_to_room {
        dc_intl::t("f7g0DK")
    } else if is_app_visible {
        dc_intl::t("3jrUBj")
    } else {
        dc_intl::t("ZtYyOx")
    }
}

/// Module 241606: icon asset name for the Voice Channel App View toggle button (`grid_square` / `apps`).
pub(crate) fn voice_channel_app_toggle_icon(
    is_app_visible: bool,
    _returns_to_room: bool,
) -> &'static str {
    if is_app_visible {
        "grid_square"
    } else {
        "apps"
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AppChannelPreview {
    pub channel_id: Id,
    pub channel_name: String,
    pub application_id: Option<Id>,
    pub app_name: String,
    pub phase: AppChannelFramePhase,
    pub update_available: bool,
    pub chat_sidebar_open: bool,
    pub toasts: Vec<AppChannelChatToast>,
}

/// Module 98492: determines whether the chat sidebar should auto-open on entering an App Channel
/// (`canHaveChat && (hasUnread || wasUnreadOnSelect) && oncePerChannel && !sidebarAlreadyOpen`).
#[allow(dead_code)]
pub(crate) fn should_auto_open_app_channel_chat(
    can_have_chat: bool,
    has_unread: bool,
    was_unread_on_select: bool,
    once_per_channel_pending: bool,
    sidebar_already_open: bool,
) -> bool {
    can_have_chat
        && (has_unread || was_unread_on_select)
        && once_per_channel_pending
        && !sidebar_already_open
}

/// Module 98492 (`378859.A`): localized `(heading, optional_description)` for non-launched states.
pub(crate) fn app_channel_state_copy(
    phase: AppChannelFramePhase,
) -> Option<(&'static str, Option<&'static str>)> {
    match phase {
        AppChannelFramePhase::RenderingElsewhere => Some((dc_intl::t("2KIDX+"), None)),
        AppChannelFramePhase::NoApplication => Some((dc_intl::t("izggZO"), None)),
        AppChannelFramePhase::DoesNotSupportSurface => Some((dc_intl::t("iUWcU/"), None)),
        AppChannelFramePhase::Error => Some((dc_intl::t("VquUff"), Some(dc_intl::t("Sd9D/R")))),
        AppChannelFramePhase::AwaitingLaunch
        | AppChannelFramePhase::Loading
        | AppChannelFramePhase::Launched => None,
    }
}

impl Root {
    /// Renders an App Channel surface (`GUILD_APP` type 21, module 98492).
    pub(super) fn render_app_channel_surface(
        &self,
        view: &AppChannelPreview,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let channel_id = view.channel_id;
        let application_id = view.application_id;
        let app_name = view.app_name.as_str();
        let phase = view.phase;
        let update_available = view.update_available;
        let chat_sidebar_open = view.chat_sidebar_open;
        let toasts = view.toasts.as_slice();
        let is_closed = self.app_channels.closed_channels.contains(&channel_id);
        let body: AnyElement = if application_id.is_none() {
            // `tU5fiM` ("No app configured") + `E94mJf`
            Self::app_channel_state_card(
                "bot_game_controller",
                dc_intl::t("tU5fiM"),
                Some(dc_intl::t("E94mJf")),
                None,
            )
        } else if is_closed {
            // Closed relaunch card (`K`): 64x64 rounded app icon + `"Launch App"` (`PPYvuv`)
            div()
                .size_full()
                .bg(color("--background-base-lower"))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(14.))
                .p(px(24.))
                .child(
                    div()
                        .size(px(64.))
                        .rounded(px(16.))
                        .bg(color("--background-brand"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("bot_game_controller", 32., "--white")),
                )
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(app_name.to_owned()),
                )
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "app-channel-launch-{channel_id}"
                        )))
                        .h(px(38.))
                        .px(px(18.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("PPYvuv"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.app_channels.closed_channels.remove(&channel_id);
                            if let Some(p) = this.app_channels.preview.as_mut()
                                && p.channel_id == channel_id
                            {
                                p.phase = AppChannelFramePhase::Launched;
                            }
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            match phase {
                AppChannelFramePhase::AwaitingLaunch | AppChannelFramePhase::Loading => div()
                    .size_full()
                    .bg(color("--background-base-lower"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(super::voice_view::spinner())
                    .into_any_element(),
                AppChannelFramePhase::Launched => {
                    let show_toasts = !chat_sidebar_open && !toasts.is_empty();
                    div()
                        .relative()
                        .size_full()
                        .bg(color("--black"))
                        .flex()
                        .flex_col()
                        .when(update_available, |col| {
                            col.child(
                                div()
                                    .id("app-channel-update-bar")
                                    .w_full()
                                    .px(px(14.))
                                    .py(px(8.))
                                    .bg(color("--background-brand"))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(format!("{app_name} — Update available")),
                                    )
                                    .child(
                                        div()
                                            .id("app-channel-reload-btn")
                                            .px(px(10.))
                                            .py(px(4.))
                                            .rounded(px(6.))
                                            .bg(rgba(0xffffff28))
                                            .cursor_pointer()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t("GDsHl+"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(p) = this.app_channels.preview.as_mut()
                                                    && p.channel_id == channel_id
                                                {
                                                    p.update_available = false;
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            )
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_h(px(0.))
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .size(px(64.))
                                        .rounded(px(16.))
                                        .bg(rgba(0x5865f233))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon("bot_game_controller", 32., "--white")),
                                )
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--white"))
                                        .child(app_name.to_owned()),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "app-channel-close-frame-{channel_id}"
                                        )))
                                        .px(px(12.))
                                        .py(px(6.))
                                        .rounded(px(6.))
                                        .bg(rgba(0xffffff1a))
                                        .cursor_pointer()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--white"))
                                        .child(dc_intl::t("cpT0Cq"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.app_channels.closed_channels.insert(channel_id);
                                            cx.notify();
                                        })),
                                ),
                        )
                        .when(show_toasts, |root_div| {
                            root_div.child(
                                div()
                                    .absolute()
                                    .top(px(48.))
                                    .right(px(12.))
                                    .w(px(304.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .children(toasts.iter().map(|toast| {
                                        let mid = toast.message_id;
                                        div()
                                            .id(SharedString::from(format!(
                                                "app-channel-toast-{mid}"
                                            )))
                                            .p(px(10.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .bg(color("--background-surface-high"))
                                            .shadow_md()
                                            .cursor_pointer()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(p) = this.app_channels.preview.as_mut()
                                                    && p.channel_id == channel_id
                                                {
                                                    p.chat_sidebar_open = true;
                                                }
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child(toast.author.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .text_color(color("--text-default"))
                                                    .child(toast.preview.clone()),
                                            )
                                    })),
                            )
                        })
                        .into_any_element()
                }
                other => {
                    let (heading, desc) = app_channel_state_copy(other)
                        .unwrap_or((dc_intl::t("VquUff"), Some(dc_intl::t("Sd9D/R"))));
                    Self::app_channel_state_card("shop_circle_error", heading, desc, None)
                }
            }
        };

        div()
            .flex_1()
            .min_h(px(0.))
            .min_w(px(0.))
            .overflow_hidden()
            .child(body)
            .into_any_element()
    }

    fn app_channel_state_card(
        glyph: &'static str,
        heading: &str,
        desc: Option<&str>,
        extra: Option<AnyElement>,
    ) -> AnyElement {
        div()
            .size_full()
            .bg(color("--background-base-lower"))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .p(px(24.))
            .child(icon(glyph, 36., "--icon-muted"))
            .child(
                div()
                    .max_w(px(380.))
                    .text_center()
                    .text_size(px(18.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--text-strong"))
                    .child(heading.to_owned()),
            )
            .children(desc.map(|d| {
                div()
                    .max_w(px(380.))
                    .text_center()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(color("--text-muted"))
                    .child(d.to_owned())
            }))
            .children(extra)
            .into_any_element()
    }

    /// Preview modal layer for App Channel states (module 98492) in capture/QA runs.
    pub(super) fn app_channel_preview_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.app_channels.preview.as_ref()?;
        let surface = self.render_app_channel_surface(p, cx);
        Some(
            deferred(
                div()
                    .id("app-channel-preview-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.app_channels.preview = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id("app-channel-preview-frame")
                            .w(px(760.))
                            .h(px(480.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lower"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(
                                div()
                                    .h(px(44.))
                                    .px(px(16.))
                                    .border_b_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(icon(
                                                "bot_game_controller",
                                                18.,
                                                "--icon-subtle",
                                            ))
                                            .child(
                                                div()
                                                    .text_size(px(15.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child(p.channel_name.clone()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("app-channel-preview-close")
                                            .size(px(28.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.app_channels.preview = None;
                                                cx.notify();
                                            }))
                                            .child(icon("modal_close", 16., "--icon-subtle")),
                                    ),
                            )
                            .child(surface),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Module 241606 (`VOICE_CHANNEL_APP_SURFACE_TOGGLE` / `418241.s`): toggles voice channel app view.
    #[allow(dead_code)]
    pub(crate) fn toggle_voice_channel_app_surface(
        &mut self,
        channel_id: Id,
        cx: &mut Context<Self>,
    ) {
        if !self.app_channels.voice_app_visible.remove(&channel_id) {
            self.app_channels.voice_app_visible.insert(channel_id);
        }
        cx.notify();
    }

    /// Module 241606 (`.controlButton_fNq-Kh`, 8px radius, 8px padding): voice channel app view toggle button.
    #[allow(dead_code)]
    pub(crate) fn voice_channel_app_toggle_button(
        &self,
        channel_id: Id,
        returns_to_room: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_app_visible = self.app_channels.voice_app_visible.contains(&channel_id);
        let tip = voice_channel_app_toggle_label(is_app_visible, returns_to_room);
        let ic = voice_channel_app_toggle_icon(is_app_visible, returns_to_room);
        let btn = div()
            .id(SharedString::from(format!("vc-app-toggle-{channel_id}")))
            .aria_label(SharedString::from(tip))
            .p(px(8.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|s| s.bg(color("--background-mod-muted")))
            .flex()
            .items_center()
            .justify_center()
            .child(icon(ic, 20., "--interactive-icon-default"))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_voice_channel_app_surface(channel_id, cx);
            }));
        self.with_tip(
            format!("vc-app-toggle-tip-{channel_id}"),
            super::tooltip::Tip::text(tip, super::tooltip::Side::Top),
            btn,
            cx,
        )
        .into_any_element()
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_app_channel(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "close" {
            self.app_channels.preview = None;
            cx.notify();
            return "app-channel: closed".to_owned();
        }
        let cid: Id = 9849201;
        self.app_channels.closed_channels.remove(&cid);
        let (app_id, phase, closed, update_available) = match arg.trim() {
            "unconfigured" => (None, AppChannelFramePhase::AwaitingLaunch, false, false),
            "relaunch" => (Some(77), AppChannelFramePhase::Launched, true, false),
            "elsewhere" => (
                Some(77),
                AppChannelFramePhase::RenderingElsewhere,
                false,
                false,
            ),
            "error" => (Some(77), AppChannelFramePhase::Error, false, false),
            _ => (Some(77), AppChannelFramePhase::Launched, false, true),
        };
        if closed {
            self.app_channels.closed_channels.insert(cid);
        }
        self.app_channels.preview = Some(AppChannelPreview {
            channel_id: cid,
            channel_name: "chess-club".to_owned(),
            application_id: app_id,
            app_name: "Chess in the Park".to_owned(),
            phase,
            update_available,
            chat_sidebar_open: false,
            toasts: vec![AppChannelChatToast {
                message_id: 1,
                author: "plü".to_owned(),
                preview: "Your turn on board 2!".to_owned(),
            }],
        });
        cx.notify();
        format!("app-channel: opened ({})", arg.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_channel_98492_state_copy_and_sidebar_guard() {
        for key in [
            "2KIDX+", "izggZO", "iUWcU/", "VquUff", "Sd9D/R", "PPYvuv", "tU5fiM", "E94mJf",
        ] {
            assert!(dc_intl::has_key(key), "missing 98492 key {key}");
        }
        assert!(should_auto_open_app_channel_chat(
            true, true, false, true, false
        ));
        assert!(!should_auto_open_app_channel_chat(
            true, true, false, true, true
        ));
        assert!(!should_auto_open_app_channel_chat(
            false, true, true, true, false
        ));
        assert_eq!(
            app_channel_state_copy(AppChannelFramePhase::RenderingElsewhere),
            Some((dc_intl::t("2KIDX+"), None))
        );
        assert_eq!(
            app_channel_state_copy(AppChannelFramePhase::Error),
            Some((dc_intl::t("VquUff"), Some(dc_intl::t("Sd9D/R"))))
        );
        assert_eq!(app_channel_state_copy(AppChannelFramePhase::Launched), None);
    }

    #[test]
    fn voice_channel_app_toggle_241606_labels_and_icons() {
        for key in ["f7g0DK", "3jrUBj", "ZtYyOx"] {
            assert!(dc_intl::has_key(key), "missing 241606 key {key}");
        }
        assert_eq!(
            voice_channel_app_toggle_label(true, true),
            dc_intl::t("f7g0DK")
        );
        assert_eq!(
            voice_channel_app_toggle_label(true, false),
            dc_intl::t("3jrUBj")
        );
        assert_eq!(
            voice_channel_app_toggle_label(false, false),
            dc_intl::t("ZtYyOx")
        );
        assert_eq!(voice_channel_app_toggle_icon(false, false), "apps");
        assert_eq!(voice_channel_app_toggle_icon(true, false), "grid_square");
    }
}
