//! "You can't react until you join!" (module 698405, `REACTIONS`): the popout above a reaction a lurker pressed
//! (`eI` of module 30492, `position: "top"`), with Join this server and "I'm not ready yet".
//!
//! `container_Kou47m`: `--background-surface-high`, 8px corners, the high elevation, 16px in, the 64px picture
//! (`/assets/dc0be2895b8884fd.svg`) beside the content (24px from it, at most 320 wide): the title (`heading-sm/
//! semibold`), "You have to join the community first before you can interact." (`body-sm/normal`, `text-default`), and
//! 16px below, the primary Join this server (loading while the join runs) and the blank "I'm not ready yet" 24px after
//! it (`--interactive-text-default`, hover and active colours). The popout closes once the server is no longer lurked.
//! The `CHAT` variant opens from the lurker chat blocker of module 678655, which is not built.

use super::{Root, color};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Bounds, Context, FontWeight, IntoElement, Pixels, anchored, deferred, div, img,
    point, prelude::*, px,
};

const PICTURE: &str = "https://discord.com/assets/dc0be2895b8884fd.svg";

pub(super) struct LurkerReaction {
    guild: Id,
    anchor: Bounds<Pixels>,
    joining: bool,
}

/// `--elevation-high` with `--elevation-stroke`, as the profile popout paints them.
fn shadow_high() -> Vec<gpui::BoxShadow> {
    vec![
        gpui::BoxShadow {
            color: gpui::hsla(0., 0., 1., 0.08),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(1.),
            inset: false,
        },
        gpui::BoxShadow {
            color: gpui::hsla(0., 0., 0., 0.24),
            offset: point(px(0.), px(12.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}

impl Root {
    /// The server a reaction in `channel` belongs to, when the account only lurks there (`isLurking` of module
    /// 30492's reaction).
    pub(super) fn lurked_guild_of(&self, channel: Id) -> Option<Id> {
        let guild = self.guild_of_channel(channel)?;
        self.stores.lurking_guilds.contains(&guild).then_some(guild)
    }

    /// `eS` of module 30492 for a lurker: the popout opens over the reaction.
    pub(super) fn open_lurker_reaction(
        &mut self,
        guild: Id,
        anchor: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.request_image(PICTURE.into(), PICTURE.into());
        self.lurker_reaction = Some(LurkerReaction {
            guild,
            anchor,
            joining: false,
        });
        cx.notify();
    }

    fn lurker_join(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.lurker_reaction.as_mut() else {
            return;
        };
        if open.joining {
            return;
        }
        open.joining = true;
        // `joinGuild(guild, {source: CHAT_INPUT_BLOCKER})`.
        let (method, route) = super::sticker_popout::join_request(open.guild);
        let _ = self.requests.send(Request::Rest {
            key: None,
            method,
            route,
            body: Some(serde_json::Value::Object(Default::default())),
        });
        cx.notify();
    }

    pub(super) fn lurker_reaction_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.lurker_reaction.as_ref()?;
        if !self.stores.lurking_guilds.contains(&open.guild) {
            return None;
        }
        let join = super::modal::content_button(
            super::modal::button(
                "lurker-reaction-join",
                dc_intl::t("9VLmlZ"),
                if open.joining {
                    super::modal::Tone::PrimaryLoading
                } else {
                    super::modal::Tone::Primary
                },
                |this, _, cx| this.lurker_join(cx),
            ),
            cx,
        );
        let cancel = div()
            .id("lurker-reaction-cancel")
            .ml(px(24.))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--interactive-text-default"))
            .hover(|style| style.text_color(color("--interactive-text-hover")))
            .active(|style| style.text_color(color("--interactive-text-active")))
            .child(dc_intl::t("2m+Sqk"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.lurker_reaction = None;
                cx.notify();
            }));
        let card = div()
            .id("lurker-reaction")
            .occlude()
            .flex()
            .items_start()
            .p(px(16.))
            .rounded(px(8.))
            .bg(color("--background-surface-high"))
            .shadow(shadow_high())
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.lurker_reaction = None;
                cx.notify();
            }))
            .child(
                div().flex_none().size(px(64.)).children(
                    self.images
                        .get(PICTURE)
                        .cloned()
                        .map(|picture| img(picture).size_full()),
                ),
            )
            .child(
                div()
                    .ml(px(24.))
                    .max_w(px(320.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("GXvlU9")),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::t("d7b1p6")),
                    )
                    .child(div().mt(px(16.)).flex().child(join).child(cancel)),
            );
        let anchor = open.anchor;
        Some(
            deferred(
                anchored()
                    .anchor(gpui::Anchor::BottomCenter)
                    .position(point(anchor.center().x, anchor.top() - px(8.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(card),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}
