//! The overlay's Chat widget (`id` of module 415173, stylesheets 722185, 835078, 276119, 727961) and the Quick
//! Actions widget (`ns` of module 415173, stylesheet 944138).
//!
//! Chat: only while unlocked (`iu`), only for an account in the `2025-11-overlay-chat` experiment. On the left the
//! chats of `Overlay-v3-Text-Chat-Store` (`dc_overlay::text_chat`): the voice channel's chat first (a 40 px channel
//! icon box, brand while open), a separator, then each tracked chat's picture. On the right the open chat: the
//! 40 px top bar (its icon, title, and the close button, or Minimize for the voice channel's chat; pressing the bar
//! moves the widget), then the channel's messages and composer. Those are the app's own: opening a chat in the overlay
//! opens it in the app, the overlay draws the app's rows (`Root::row`) in a list of its own and the app's composer
//! (`composer_bar`), so sending goes through the one send path (refused in an inert run).
//!
//! Quick Actions: only while unlocked (`locked_6Ln5ei` is `display: none`): three button sections, Mute, Deafen and
//! Camera; Go Live and Soundboard; Connect or Disconnect (the section red while in a call).

use super::Root;
use super::overlay::onyx;
use dc_overlay::layout::{self, Widget};
use dc_overlay::text_chat::{self, ChannelInfo, Mount, PrivateChannel, Source};
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px, rgba, svg,
};

/// `rgba(19,19,24,.95)` of `chatArea_0AA5nk` and `topBar_pU9D6x`.
const CHAT_BG: u32 = 0x131318f2;

/// A snowflake's time in Unix milliseconds.
pub(super) fn snowflake_ms(id: Id) -> f64 {
    ((id >> 22) + 1_420_070_400_000) as f64
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0., |since| since.as_millis() as f64)
}

/// Quick Actions tray button size (`609142.js` / `491899.db7bf5e846a02ae2.css`: 20px icon + 8px padding).
pub(super) const QUICK_ACTIONS_BUTTON_SIZE: f32 = 36.;

/// Outer `wrapper_yjvjn7` height of the Quick Actions widget: 36px button + 10px section box + 18px panel box +
/// 20px drag padding/border = 84px.
#[cfg(test)]
pub(super) const QUICK_ACTIONS_WRAPPER_HEIGHT: f32 = QUICK_ACTIONS_BUTTON_SIZE
    + 10.
    + 18.
    + 2. * (dc_overlay::drag::PADDING + dc_overlay::drag::BORDER);

impl Root {
    /// What `Overlay-v3-Text-Chat-Store` reads about a channel.
    pub(super) fn overlay_channel_info(&self, channel: Id) -> ChannelInfo {
        let private = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel);
        let last = private
            .map(|known| known.last_message_id)
            .filter(|id| *id != 0)
            .or_else(|| {
                self.stores
                    .guild_of_channel(channel)
                    .and_then(|guild| guild.channels.iter().find(|known| known.id == channel))
                    .and_then(|known| known.last_message_id)
            });
        ChannelInfo {
            is_private: private.is_some(),
            mention_count: self.stores.read_states.mentions(channel) as u32,
            has_unread: self.stores.is_unread(channel),
            last_message_ms: last.map(snowflake_ms),
        }
    }

    /// `OVERLAY_MOUNTED` for the text chat store (once per overlay).
    pub(super) fn overlay_text_chat_mount(&mut self) {
        let now = now_ms();
        let app_channel = self
            .channel
            .map(|channel| (channel, self.overlay_channel_info(channel)));
        let utc = chrono::Utc::now();
        let private_channels = self
            .stores
            .private_channels
            .iter()
            .map(|known| PrivateChannel {
                channel: known.id,
                muted: self.stores.guild_settings.channel_muted(0, known.id, utc),
                message_request: known.message_request,
                info: ChannelInfo {
                    is_private: true,
                    mention_count: self.stores.read_states.mentions(known.id) as u32,
                    has_unread: self.stores.is_unread(known.id),
                    last_message_ms: (known.last_message_id != 0)
                        .then(|| snowflake_ms(known.last_message_id)),
                },
            })
            .collect::<Vec<_>>();
        // `getPrivateChannelIds()` is newest first.
        let mut private_channels = private_channels;
        private_channels.sort_by(|a, b| {
            b.info
                .last_message_ms
                .partial_cmp(&a.info.last_message_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mount = Mount {
            now_ms: now,
            app_channel,
            voice_channel: self.call.as_ref().map(|call| call.channel),
            private_channels,
        };
        if self.overlay.text_chat.mount(&mount) {
            tracing::debug!(
                "overlay chat: {} chats tracked",
                self.overlay.text_chat.entries().0.len()
            );
        }
    }

    /// `D$` with a channel target: the chat opens in the overlay (and in the app, whose rows and composer it draws).
    pub(super) fn overlay_chat_select(&mut self, channel: Id, cx: &mut Context<Self>) {
        let info = self.overlay_channel_info(channel);
        let voice = self.call.as_ref().map(|call| call.channel);
        self.overlay
            .text_chat
            .select(channel, Source::Manual, info, now_ms(), voice);
        // `JA(TEXT_CHAT_V3, false)`: the widget is shown.
        self.overlay_change_layout(|layout| {
            if layout.is_enabled(layout::TEXT_CHAT) {
                return false;
            }
            layout.set_enabled(layout::TEXT_CHAT, true, &mut super::overlay::new_widget_id);
            true
        });
        if self.channel != Some(channel) {
            self.open_notification_channel(channel, cx);
        }
    }

    /// A chat's title (`m1`): a DM's or group's name, a channel's name.
    fn overlay_chat_title(&self, channel: Id) -> String {
        if let Some(known) = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel)
        {
            return self.private_channel_name(known);
        }
        self.stores
            .guild_of_channel(channel)
            .and_then(|guild| guild.channels.iter().find(|known| known.id == channel))
            .map(|known| known.name.clone())
            .unwrap_or_else(|| dc_intl::t("zLZPmk").to_owned())
    }

    /// A rail item's 40 px picture: a DM's avatar, else a rounded box with the first letter.
    fn overlay_chat_picture(&mut self, channel: Id, selected: bool) -> AnyElement {
        let user = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel && known.kind == 1)
            .and_then(|known| known.recipients.first())
            .and_then(|id| self.stores.users.get(id))
            .cloned();
        if let Some(user) = user {
            let (key, url) = super::avatar_source(&user);
            if !self.images.contains_key(&key) {
                self.request_image(key.clone(), url);
            }
            if let Some(image) = self.images.get(&key).cloned() {
                return img(image).size(px(40.)).rounded_full().into_any_element();
            }
        }
        let letter = self
            .overlay_chat_title(channel)
            .chars()
            .next()
            .map(String::from)
            .unwrap_or_default();
        div()
            .size(px(40.))
            .rounded(px(12.))
            .flex()
            .items_center()
            .justify_center()
            .bg(if selected {
                onyx("--background-brand")
            } else {
                onyx("--background-surface-high")
            })
            .text_size(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(onyx("--white"))
            .child(letter)
            .into_any_element()
    }

    /// `n8`: one chat of the rail.
    fn overlay_chat_nav(&mut self, channel: Id, voice: bool, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.overlay.text_chat.selected() == Some(channel);
        let picture = if voice {
            // `tivNavItemIconContainer`: the channel-type icon in a 40 px box, brand at 90% while open.
            div()
                .size(px(40.))
                .rounded(px(12.))
                .flex()
                .items_center()
                .justify_center()
                .bg(if selected {
                    gpui::Rgba {
                        a: 0.9,
                        ..onyx("--brand-500")
                    }
                } else {
                    onyx("--background-surface-high")
                })
                .child(
                    svg()
                        .path("icons/overlay_voice_normal.svg")
                        .size(px(20.))
                        .text_color(if selected {
                            onyx("--white")
                        } else {
                            onyx("--icon-status-online")
                        }),
                )
                .into_any_element()
        } else {
            self.overlay_chat_picture(channel, selected)
        };
        div()
            .id(SharedString::from(format!("overlay-chat-nav-{channel}")))
            .relative()
            .pt(px(8.))
            .pb(px(8.))
            .pl(px(16.))
            .pr(px(12.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                if voice
                    && this.overlay.text_chat.voice_chat_minimized()
                    && let Some(call) = this.call.as_ref().map(|call| call.channel)
                {
                    this.overlay
                        .text_chat
                        .set_voice_chat_minimized(false, Some(call));
                }
                this.overlay_chat_select(channel, cx);
                cx.notify();
            }))
            .child(picture)
            // `animatedHalfPillContainer`: the open chat's pill at the left edge.
            .when(selected, |this| {
                this.child(
                    div()
                        .absolute()
                        .left(px(2.))
                        .top(px(8. + 10.))
                        .w(px(4.))
                        .h(px(20.))
                        .rounded_r(px(4.))
                        .bg(onyx("--white")),
                )
            })
            .into_any_element()
    }

    /// The Chat widget's content at the widget's size.
    pub(super) fn overlay_text_chat(
        &mut self,
        widget: &Widget,
        window_width: f32,
        window_height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let voice = self.call.as_ref().map(|call| call.channel);
        self.overlay.text_chat.voice_changed(voice);
        let rail: Vec<Id> = text_chat::rail(&self.overlay.text_chat, voice);
        let (width, height) = dc_overlay::drag::requested_size(
            widget.size.pixels(window_width, window_height),
            widget.min_size,
        );
        let inner = |value: Option<f32>| {
            value
                .map(|value| value - 2. * dc_overlay::drag::PADDING - 2. * dc_overlay::drag::BORDER)
        };
        let (width, height) = (inner(width), inner(height));
        if rail.is_empty() {
            // `q.g`: "Select a chat to start messaging." under the chat icon.
            return div()
                .when_some(width, |this, width| this.w(px(width)))
                .when_some(height, |this, height| this.h(px(height)))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(
                    svg()
                        .path("icons/overlay_chat.svg")
                        .size(px(24.))
                        .text_color(onyx("--white"))
                        .opacity(0.24),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(onyx("--text-muted"))
                        .child(dc_intl::tr("Select a chat to start messaging.")),
                )
                .into_any_element();
        }
        let mut nav = Vec::new();
        for (index, channel) in rail.iter().enumerate() {
            let is_voice = Some(*channel) == voice;
            nav.push(self.overlay_chat_nav(*channel, is_voice, cx));
            if is_voice && index == 0 && rail.len() > 1 {
                nav.push(
                    div()
                        .ml(px(20.))
                        .my(px(2.))
                        .w(px(32.))
                        .h(px(1.))
                        .bg(onyx("--background-mod-strong"))
                        .opacity(0.8)
                        .into_any_element(),
                );
            }
        }
        let selected = self.overlay.text_chat.selected();
        let pane = text_chat::pane_shown(&self.overlay.text_chat, voice)
            .then_some(selected)
            .flatten()
            .map(|channel| self.overlay_chat_pane(widget, channel, Some(channel) == voice, cx));
        div()
            .when_some(width, |this, width| this.w(px(width + 10.)))
            .when_some(height, |this, height| this.h(px(height - 6.)))
            .flex()
            .flex_row()
            .items_stretch()
            .ml(px(-12.))
            .pt(px(4.))
            .pb(px(4.))
            .child(
                div()
                    .id("overlay-chat-rail")
                    .flex_none()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .overflow_y_scroll()
                    .children(nav),
            )
            .children(pane)
            .into_any_element()
    }

    /// The open chat: the top bar, then the app's rows and composer for it.
    fn overlay_chat_pane(
        &mut self,
        widget: &Widget,
        channel: Id,
        voice: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = self.overlay_chat_title(channel);
        let id = widget.id.clone();
        let min_size = widget.min_size;
        let header = div()
            .id("overlay-chat-top-bar")
            .flex_none()
            .h(px(40.))
            .pt(px(4.))
            .px(px(12.))
            .pb(px(8.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .bg(rgba(CHAT_BG))
            .border_b_1()
            .border_color(onyx("--background-mod-strong"))
            .cursor_grab()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if !this.overlay.locked {
                        this.overlay_drag_start(
                            &id,
                            min_size,
                            event.position,
                            dc_overlay::drag::Operation::Move,
                        );
                    }
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(onyx("--text-strong"))
                    .child(title),
            )
            .child(
                div()
                    .id("overlay-chat-close")
                    .size(px(32.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|style| style.bg(onyx("--interactive-background-hover")))
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                        let call = this.call.as_ref().map(|call| call.channel);
                        if voice {
                            this.overlay.text_chat.set_voice_chat_minimized(true, call);
                        } else if event.modifiers().shift {
                            this.overlay.text_chat.remove_all(call);
                        } else {
                            this.overlay.text_chat.remove(channel, call);
                        }
                        if let Some(next) = this.overlay.text_chat.selected() {
                            this.overlay_chat_select(next, cx);
                        }
                        cx.notify();
                    }))
                    .child(
                        svg()
                            .path(if voice {
                                "icons/overlay_minimize.svg"
                            } else {
                                "icons/overlay_x_small_sm.svg"
                            })
                            .size(px(20.))
                            .text_color(onyx("--interactive-text-default")),
                    ),
            );
        // The app's rows of the open channel, in the overlay's own list (the main window lays out its own).
        let current = self.channel == Some(channel);
        let built = (self.channel, self.rows.len());
        if self.overlay.chat_rows != built {
            self.overlay.chat_rows = built;
            self.overlay.chat_list.reset(self.rows.len());
        }
        let rows = current.then(|| {
            gpui::list(
                self.overlay.chat_list.clone(),
                cx.processor(|this, index, _, cx| this.row(index, cx)),
            )
            .size_full()
        });
        div()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .flex()
            .flex_col()
            .relative()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(onyx("--border-subtle"))
            .bg(rgba(CHAT_BG))
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_col()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(div().flex_1().min_h(px(0.)).children(rows))
                    .when(current, |this| this.child(self.composer_bar(cx))),
            )
            .into_any_element()
    }
}

// ----- quick actions -----

/// A tray button's colour (`color` of module 693409): `primaryDark`, `red`, `green`, `disconnect`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tint {
    Default,
    Red,
    Green,
    Disconnect,
}

/// One quick action as drawn: its icon, colour and whether it can be pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Tray {
    pub id: &'static str,
    pub icon: &'static str,
    pub tint: Tint,
    pub disabled: bool,
}

/// What the buttons read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct QuickFacts {
    pub in_voice: bool,
    pub self_mute: bool,
    pub server_mute: bool,
    pub self_deaf: bool,
    pub server_deaf: bool,
    pub camera: bool,
    pub streaming: bool,
}

/// `t5`, `t6`, `t4` / `t8`, `t9` / `t7`: the three sections of buttons.
pub(super) fn quick_actions(facts: QuickFacts) -> [Vec<Tray>; 3] {
    let muted = facts.self_mute || facts.server_mute;
    let deaf = facts.self_deaf || facts.server_deaf;
    let tray = |id, icon, tint, disabled| Tray {
        id,
        icon,
        tint,
        disabled,
    };
    [
        vec![
            tray(
                "overlay-quick-mute",
                if muted {
                    "overlay_microphone_slash"
                } else {
                    "overlay_microphone"
                },
                if muted { Tint::Red } else { Tint::Default },
                false,
            ),
            tray(
                "overlay-quick-deafen",
                if facts.server_deaf {
                    "overlay_headphones_deny"
                } else if deaf {
                    "overlay_headphones_slash"
                } else {
                    "overlay_headphones"
                },
                if deaf { Tint::Red } else { Tint::Default },
                false,
            ),
            tray(
                "overlay-quick-camera",
                if facts.camera {
                    "overlay_video"
                } else {
                    "overlay_video_slash"
                },
                if facts.camera {
                    Tint::Green
                } else {
                    Tint::Default
                },
                !facts.in_voice,
            ),
        ],
        vec![
            tray(
                "overlay-quick-go-live",
                "overlay_screen_arrow",
                if facts.streaming {
                    Tint::Green
                } else {
                    Tint::Default
                },
                !facts.in_voice,
            ),
            tray(
                "overlay-quick-soundboard",
                "overlay_soundboard",
                Tint::Default,
                !facts.in_voice || facts.server_mute || deaf,
            ),
        ],
        vec![tray(
            "overlay-quick-call",
            if facts.in_voice {
                "overlay_phone_hang_up"
            } else {
                "overlay_phone_call"
            },
            if facts.in_voice {
                Tint::Disconnect
            } else {
                Tint::Default
            },
            false,
        )],
    ]
}

impl Root {
    fn overlay_quick_facts(&self) -> QuickFacts {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let state = self.call.as_ref().and_then(|call| {
            self.stores
                .voice_members(call.guild, call.channel)
                .into_iter()
                .find(|state| Some(state.user_id) == me)
                .cloned()
        });
        QuickFacts {
            in_voice: self.call.is_some(),
            self_mute: self.self_mute,
            server_mute: state
                .as_ref()
                .is_some_and(|state| state.mute || state.suppress),
            self_deaf: self.self_deaf,
            server_deaf: state.as_ref().is_some_and(|state| state.deaf),
            camera: self.call.as_ref().is_some_and(|call| call.camera),
            streaming: state.as_ref().is_some_and(|state| state.self_stream),
        }
    }

    fn overlay_quick_pressed(&mut self, id: &str, cx: &mut Context<Self>) {
        match id {
            "overlay-quick-mute" => self.toggle_self_mute(cx),
            "overlay-quick-deafen" => self.toggle_self_deaf(cx),
            "overlay-quick-camera" => self.voice_camera_clicked(cx),
            "overlay-quick-go-live" => self.voice_screen_clicked(cx),
            "overlay-quick-call" if self.call.is_some() => self.leave_voice(cx),
            // The soundboard and the voice channel picker are popouts that are not built.
            _ => {}
        }
    }

    /// `ns` of module 415173.
    pub(super) fn overlay_quick_actions(&self, locked: bool, cx: &mut Context<Self>) -> AnyElement {
        if locked {
            return div().into_any_element();
        }
        let facts = self.overlay_quick_facts();
        let sections = quick_actions(facts);
        let in_voice = facts.in_voice;
        let section_count = sections.len();
        let section = |index: usize, buttons: Vec<Tray>, cx: &mut Context<Self>| {
            div()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .p(px(4.))
                .rounded(px(12.))
                .border_1()
                .border_color(onyx("--border-muted"))
                .bg(if in_voice && index + 1 == section_count {
                    onyx("--background-feedback-mod-critical")
                } else {
                    onyx("--background-mod-subtle")
                })
                .children(buttons.into_iter().map(|tray| {
                    let (icon, background, hover) = match tray.tint {
                        Tint::Default => ("--white", None, Some(gpui::hsla(0., 0., 1., 0.1))),
                        Tint::Red => ("--red-400", Some(onyx("--opacity-red-12")), None),
                        Tint::Green => ("--green-300", None, Some(gpui::hsla(0., 0., 1., 0.1))),
                        Tint::Disconnect => (
                            "--control-critical-primary-text-default",
                            Some(onyx("--control-critical-primary-background-default")),
                            None,
                        ),
                    };
                    let id = tray.id;
                    div()
                        .id(id)
                        .size(px(QUICK_ACTIONS_BUTTON_SIZE))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.))
                        .when_some(background, |this, background| this.bg(background))
                        .when(!tray.disabled, |this| {
                            this.cursor_pointer()
                                .when_some(hover, |this, hover| {
                                    this.hover(move |style| style.bg(hover))
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.overlay_quick_pressed(id, cx);
                                    cx.notify();
                                }))
                        })
                        .when(tray.disabled, |this| this.opacity(0.5))
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            svg()
                                .path(format!("icons/{}.svg", tray.icon))
                                .size(px(20.))
                                .text_color(onyx(icon)),
                        )
                }))
        };
        let [first, second, third] = sections;
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .p(px(8.))
            .rounded(px(16.))
            .border_1()
            .border_color(onyx("--border-subtle"))
            .bg(onyx("--black"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(12.))
                    .child(section(0, first, cx))
                    .child(section(1, second, cx))
                    .child(section(2, third, cx)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_actions_follow_the_call() {
        let idle = quick_actions(QuickFacts::default());
        assert_eq!(idle[0][0].icon, "overlay_microphone");
        assert!(idle[0][2].disabled, "no camera without a call");
        assert!(idle[1].iter().all(|tray| tray.disabled));
        assert_eq!(idle[2][0].icon, "overlay_phone_call");
        assert_eq!(idle[2][0].tint, Tint::Default);
        let call = quick_actions(QuickFacts {
            in_voice: true,
            self_mute: true,
            server_deaf: true,
            camera: true,
            ..QuickFacts::default()
        });
        assert_eq!(call[0][0].tint, Tint::Red);
        assert_eq!(call[0][0].icon, "overlay_microphone_slash");
        assert_eq!(call[0][1].icon, "overlay_headphones_deny");
        assert_eq!(call[0][2].tint, Tint::Green);
        assert!(call[1][1].disabled, "no soundboard while deafened");
        assert_eq!(call[2][0].tint, Tint::Disconnect);
        assert_eq!(call[2][0].icon, "overlay_phone_hang_up");
    }

    #[test]
    fn a_snowflake_reads_as_its_time() {
        // 175928847299117063 is 2016-04-30 11:18:25.796 UTC.
        assert_eq!(snowflake_ms(175_928_847_299_117_063), 1_462_015_105_796.);
    }
}
