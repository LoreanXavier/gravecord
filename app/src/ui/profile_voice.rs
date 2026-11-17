//! The "In voice" card of a profile popout (`design/dom/voice-user-menu/row-left-click-darker-cozy.json`): the clicked
//! person's voice channel as a `container_M2WrKU` > `overlay_P1YEjQ` card with the avatar tile, the channel, the server,
//! the channel status line and an "Open Voice" button.

use super::{Root, avatar_source, color, icon};
use dc_state::{ChannelKind, Id};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

impl Root {
    /// The card for someone in a voice channel of the open server; `None` when they are not in one.
    pub(super) fn popout_voice_card(&self, user: Id, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild_id = self.guild?;
        let state = self
            .stores
            .voice_states
            .get(&guild_id)?
            .iter()
            .find(|state| state.user_id == user)?;
        let guild = self.stores.guild(guild_id)?;
        let channel = guild
            .channels
            .iter()
            .find(|channel| channel.id == state.channel_id)?;
        if !matches!(channel.kind, ChannelKind::Voice | ChannelKind::Other(13)) {
            return None;
        }
        let person = self
            .stores
            .users
            .get(&user)
            .cloned()
            .or_else(|| {
                self.stores
                    .current_user
                    .clone()
                    .filter(|current| current.id == user)
            })
            .unwrap_or_default();
        let avatar = self.images.get(&avatar_source(&person).0).cloned();
        let status = self.voice_channel_status(channel.id).map(str::to_owned);
        // `r4`: with no status, "Set a channel status" for someone who may set it, only while this client is in it.
        let can_set = self.in_voice_channel(channel.id) && self.can_set_voice_status(channel);
        let (guild_name, channel_name, target) =
            (guild.name.clone(), channel.name.clone(), channel.id);
        // Module 70963: the button needs Connect; it opens the channel this client is in and joins any other.
        let joined = self.in_voice_channel(channel.id);
        let can_connect = self.can_in_channel(channel, dc_state::permissions::CONNECT);
        let stage = channel.kind == ChannelKind::Other(13);
        let label = voice_button_label(joined, stage);
        let strong = color("--text-strong");
        let subtle = color("--text-subtle");
        let line = |text: String, tint: gpui::Rgba| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .h(px(16.))
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(tint)
                .child(text)
        };
        Some(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .p(px(8.))
                        .rounded(px(8.))
                        .bg(color("--user-profile-overlay-background"))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        // `header_Bu4Al8`: the title with its 12px info icon, and the 16px more button.
                        .child(
                            div()
                                .h(px(16.))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child("In voice")
                                        .child(icon("circle_info", 12., "--text-subtle")),
                                )
                                .child(icon("more_horizontal", 16., "--text-subtle")),
                        )
                        // `content_Bu4Al8`: a 60px tile with the 28px avatar, then the details.
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .size(px(60.))
                                        .flex_none()
                                        .rounded(px(8.))
                                        // `container_vg4rr1` reads rgb(50, 50, 55) in the capture's frame.
                                        .bg(gpui::rgb(0x323237))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .size(px(28.))
                                                .rounded_full()
                                                .overflow_hidden()
                                                .children(avatar.map(|picture| {
                                                    img(picture).size(px(28.)).rounded_full()
                                                })),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .flex()
                                        .flex_col()
                                        .justify_center()
                                        .gap(px(4.))
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .child(
                                                    div()
                                                        .h(px(18.))
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(2.))
                                                        .text_size(px(14.))
                                                        .line_height(px(18.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(strong)
                                                        .child(icon(
                                                            "channel_type_voice",
                                                            12.,
                                                            "--text-strong",
                                                        ))
                                                        .child(channel_name),
                                                )
                                                .child(line(format!("in {guild_name}"), strong)),
                                        )
                                        .when_some(status.clone(), |this, text| {
                                            this.child(line(text, subtle))
                                        })
                                        .when(status.is_none() && can_set, |this| {
                                            this.child(
                                                line("Set a channel status".to_owned(), subtle)
                                                    .child(icon(
                                                        "channel_status_pencil",
                                                        12.,
                                                        "--text-subtle",
                                                    )),
                                            )
                                        }),
                                ),
                        )
                        // `actions_Bu4Al8`: the 4px-margin button row (module 70963): Open Voice/Stage for the
                        // channel this client is in, Join Voice/Stage for another; none without Connect.
                        .when(can_connect, |this| this.child(
                            div().mt(px(4.)).child(
                                div()
                                    .id(SharedString::from(format!("popout-open-voice-{target}")))
                                    .cursor_pointer()
                                    .h(px(32.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .child(label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.profile = None;
                                        if joined {
                                            this.select(guild_id, Some(target));
                                        } else {
                                            this.join_voice(guild_id, target, cx);
                                        }
                                        cx.notify();
                                    })),
                            ),
                        )),
                )
                .into_any_element(),
        )
    }
}

/// The label of module 70963 outside a modal: Open or Join, Voice or Stage (a guild channel here; DMs say Call).
fn voice_button_label(joined: bool, stage: bool) -> &'static str {
    match (joined, stage) {
        (true, false) => dc_intl::t("BXxdl7"),
        (true, true) => dc_intl::t("Acqcot"),
        (false, false) => dc_intl::t("96ANUN"),
        (false, true) => dc_intl::t("7vb2cc"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_button_opens_the_channel_this_client_is_in_and_joins_another() {
        assert_eq!(super::voice_button_label(true, false), "Open Voice");
        assert_eq!(super::voice_button_label(false, false), "Join Voice");
        assert_eq!(super::voice_button_label(true, true), "Open Stage");
        assert_eq!(super::voice_button_label(false, true), "Join Stage");
    }
}
