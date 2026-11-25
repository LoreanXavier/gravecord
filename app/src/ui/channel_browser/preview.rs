//! Browser chat sidebar: 624186, 108926, 812771 and 964404.
use crate::{
    net::Request,
    parity::Parity,
    ui::{
        Root, color, icon,
        tooltip::{Side, Tip},
    },
};
use dc_state::{
    Id,
    guild_settings::{ALL_MESSAGES, INHERIT},
};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseButton, MouseMoveEvent, SharedString, div,
    prelude::*, px,
};

fn resized_width(start: (f32, f32), pointer: f32) -> f32 {
    (start.1 + start.0 - pointer).clamp(450., 600.)
}

impl Root {
    pub(super) fn activate_browser_channel(
        &mut self,
        guild: Id,
        channel: Id,
        full: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(known) = self
            .stores
            .guild(guild)
            .and_then(|guild| guild.channels.iter().find(|known| known.id == channel))
        else {
            return;
        };
        if full || known.is_forum() {
            self.select(guild, Some(channel));
            self.request_images();
            cx.notify();
        } else if matches!(
            known.kind,
            dc_state::ChannelKind::Voice | dc_state::ChannelKind::Other(13)
        ) {
            self.request_join_voice(guild, channel, cx);
        } else {
            self.open_browser_preview(channel, cx);
        }
    }

    pub(super) fn open_browser_preview(&mut self, channel: Id, cx: &mut Context<Self>) {
        let Some(guild) = self.guild.filter(|_| self.channel_browser_open) else {
            return;
        };
        self.channel = Some(channel);
        self.unread_from = None;
        self.notify_open = false;
        self.notify_submenu = None;
        self.subscribe_selected_channel(guild, channel);
        let _ = self.requests.send(Request::LoadMessages { channel });
        self.rebuild_rows();
        self.request_images();
        cx.notify();
    }

    fn close_browser_preview(&mut self, cx: &mut Context<Self>) {
        self.channel = None;
        self.notify_open = false;
        self.notify_submenu = None;
        self.tip = None;
        if let Some(browser) = self.channel_browser.as_mut() {
            browser.resize_from = None;
        }
        self.rebuild_rows();
        cx.notify();
    }

    pub(in crate::ui) fn browser_preview_width(&self) -> f32 {
        self.channel_browser
            .as_ref()
            .map_or(480., |browser| browser.preview_width)
    }

    pub(in crate::ui) fn move_browser_resize(
        &mut self,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        if !self.channel_browser_open || !event.dragging() {
            return;
        }
        if let Some(browser) = self.channel_browser.as_mut()
            && let Some(start) = browser.resize_from
        {
            browser.preview_width = resized_width(start, f32::from(event.position.x));
            cx.notify();
        }
    }

    pub(in crate::ui) fn end_browser_resize(&mut self) {
        if let Some(browser) = self.channel_browser.as_mut()
            && browser.resize_from.take().is_some()
            && !crate::storage::settings_writes_blocked()
        {
            crate::storage::set(
                "threadSidebarWidth",
                serde_json::json!(browser.preview_width),
            );
        }
    }

    pub(super) fn browser_resize_handle(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("browser-preview-resize")
            .w(px(8.))
            .h_full()
            .flex_none()
            .cursor_ew_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    if let Some(browser) = this.channel_browser.as_mut() {
                        browser.resize_from =
                            Some((f32::from(event.position.x), browser.preview_width));
                    }
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }

    pub(super) fn browser_preview(
        &self,
        messages: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (Some(guild), Some(channel)) = (self.guild, self.current_channel()) else {
            return div().into_any_element();
        };
        let channel_id = channel.id;
        let own = self.stores.guild_settings.channel(guild, channel_id);
        let level = if own.message_notifications == INHERIT {
            self.stores
                .guild_settings
                .inherited(guild, channel.parent_id)
        } else {
            own.message_notifications
        };
        let bell = if self.channel_muted_now() || level != ALL_MESSAGES {
            "bell_muted"
        } else {
            "bell"
        };
        let tool = |name: &str| {
            let group = SharedString::from(format!("browser-preview-{name}"));
            let active = name == bell && self.notify_open;
            div()
                .id(group.clone())
                .group(group.clone())
                .size(px(32.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .child(
                    gpui::svg()
                        .path(format!("icons/{name}.svg"))
                        .size(px(20.))
                        .text_color(color(if active {
                            "--interactive-text-active"
                        } else {
                            "--text-muted"
                        }))
                        .group_hover(group, |style| {
                            style.text_color(color(if active {
                                "--interactive-text-active"
                            } else {
                                "--interactive-text-default"
                            }))
                        }),
                )
        };
        div()
            .probe("browser-preview", [1., 1., 0., 1.])
            .w(px(self.browser_preview_width()))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded_tl(px(12.))
            .border_t_1()
            .border_l_1()
            .border_r_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-lower"))
            .child(
                div()
                    .parity_below("browser-preview-header", 1.)
                    .h(px(49.))
                    .flex_none()
                    .pl(px(16.))
                    .pr(px(8.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon(
                        if self
                            .current_guild()
                            .and_then(|guild| guild.rules_channel_id)
                            == Some(channel_id)
                        {
                            "ob_rules"
                        } else {
                            "header_hash"
                        },
                        20.,
                        "--icon-muted",
                    ))
                    .child(
                        div()
                            .id("browser-preview-title")
                            .min_w(px(0.))
                            .text_ellipsis()
                            .overflow_hidden()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .cursor_pointer()
                            .child(channel.name.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select(guild, Some(channel_id));
                                this.request_images();
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        self.with_tip(
                            "notifications",
                            Tip::text("Notification Settings", Side::Bottom),
                            tool(bell),
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.tip = None;
                            this.notify_open = !this.notify_open;
                            this.notify_submenu = None;
                            cx.notify();
                        })),
                    )
                    .child(
                        self.with_tip(
                            "browser-preview-close",
                            Tip::text("Close", Side::Bottom),
                            tool("ob_x"),
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.close_browser_preview(cx))),
                    ),
            )
            .child(
                div()
                    .parity("browser-preview-messages")
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .child(messages)
                    .children(self.new_messages_bar(cx)),
            )
            .child(if self.screening_pending() {
                self.screening_composer(cx)
            } else {
                self.composer_bar(cx).into_any_element()
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dragging_left_grows_the_sidebar_within_source_limits() {
        assert_eq!(resized_width((796., 480.), 766.), 510.);
        assert_eq!(resized_width((796., 480.), 0.), 600.);
        assert_eq!(resized_width((796., 480.), 1200.), 450.);
    }
}
