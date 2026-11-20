//! Soundmoji popout, Guild Info card, hover preview, and Volume slider (module 456086).

use super::{Root, color, icon};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SoundmojiButton {
    JoinGuild,
    GetNitro,
    None,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct SoundmojiContext {
    pub sound_name: String,
    pub sound_emoji: String,
    pub is_default_sound: bool,
    pub can_send_soundmojis: bool,
    pub has_soundmoji_permissions: bool,
    pub is_in_guild: bool,
    pub is_guild_discoverable: bool,
    pub is_sound_from_different_guild: bool,
    pub source_guild_name: Option<String>,
    pub volume_pct: u8,
    pub show_volume: bool,
}

/// Official `buttonType` & exhaustive `description` matrix (`B` of module 456086).
pub(crate) fn soundmoji_info(ctx: &SoundmojiContext) -> (SoundmojiButton, &'static str) {
    let button = if ctx.is_default_sound || !ctx.can_send_soundmojis {
        SoundmojiButton::None
    } else if ctx.has_soundmoji_permissions {
        if ctx.is_in_guild || !ctx.is_guild_discoverable {
            SoundmojiButton::None
        } else {
            SoundmojiButton::JoinGuild
        }
    } else {
        SoundmojiButton::GetNitro
    };

    let desc_key = if !ctx.can_send_soundmojis {
        "x2kyyJ"
    } else if ctx.is_default_sound {
        "AabHep"
    } else if !ctx.is_in_guild && !ctx.is_guild_discoverable {
        "MRYt06"
    } else if ctx.has_soundmoji_permissions {
        if ctx.is_in_guild && !ctx.is_sound_from_different_guild {
            "p17MQJ"
        } else if ctx.is_in_guild {
            "Lkbm5s"
        } else {
            "GTJmaS"
        }
    } else if ctx.is_in_guild && !ctx.is_sound_from_different_guild {
        "sj/imS"
    } else if ctx.is_in_guild {
        "3Ru2/x"
    } else {
        "qRkWhZ"
    };

    (button, dc_intl::t(desc_key))
}

impl Root {
    pub(crate) fn close_soundmoji_popout(&mut self) -> bool {
        self.soundmoji_popout.take().is_some()
    }

    pub(in crate::ui) fn soundmoji_popout_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let ctx = self.soundmoji_popout.as_ref()?;
        let (btn_kind, description) = soundmoji_info(ctx);
        let show_nitro_banner = !ctx.has_soundmoji_permissions
            && !ctx.is_default_sound
            && ctx.is_sound_from_different_guild
            && ctx.can_send_soundmojis;

        let banner: AnyElement = if show_nitro_banner {
            div()
                .px(px(14.))
                .py(px(8.))
                .bg(color("--control-primary-background-default"))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(icon("nitro_wheel", 14., "--white"))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("BMw+7I")),
                        ),
                )
                .child(
                    div()
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .bg(rgba(0x00000044))
                        .text_size(px(10.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--white"))
                        .child("BETA"),
                )
                .into_any_element()
        } else {
            div()
                .px(px(14.))
                .pt(px(12.))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .bg(color("--control-primary-background-default"))
                        .text_size(px(10.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--white"))
                        .child("BETA"),
                )
                .child(
                    div()
                        .id("soundmoji-close")
                        .size(px(24.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("modal_close", 14., "--interactive-icon-default"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_soundmoji_popout();
                            cx.notify();
                        })),
                )
                .into_any_element()
        };

        let cta = match btn_kind {
            SoundmojiButton::GetNitro => Some(
                div()
                    .id("soundmoji-get-nitro")
                    .h(px(34.))
                    .w_full()
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .text_size(px(13.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--white"))
                    .child(icon("nitro_wheel", 14., "--white"))
                    .child(dc_intl::t("pj0XBN"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_soundmoji_popout();
                        this.open_nitro(cx);
                    })),
            ),
            SoundmojiButton::JoinGuild => Some(
                div()
                    .id("soundmoji-join-guild")
                    .h(px(34.))
                    .w_full()
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--white"))
                    .child(dc_intl::t("riu2R5"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_soundmoji_popout();
                        cx.notify();
                    })),
            ),
            SoundmojiButton::None => None,
        };

        let vol = ctx.volume_pct.min(100);
        let volume_panel = ctx.show_volume.then(|| {
            div()
                .p(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-lower"))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("2JbvKw")),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(color("--text-muted"))
                                .child(format!("{vol}%")),
                        ),
                )
                .child(div().flex().items_center().gap(px(6.)).children(
                    [0u8, 25, 50, 75, 100].into_iter().map(|step| {
                        let active = vol == step;
                        div()
                            .id(SharedString::from(format!("soundmoji-vol-{step}")))
                            .flex_1()
                            .h(px(24.))
                            .rounded(px(6.))
                            .bg(if active {
                                color("--control-primary-background-default")
                            } else {
                                color("--control-secondary-background-default")
                            })
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(11.))
                            .text_color(if active {
                                color("--white")
                            } else {
                                color("--text-default")
                            })
                            .child(format!("{step}%"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(s) = this.soundmoji_popout.as_mut() {
                                    s.volume_pct = step;
                                }
                                cx.notify();
                            }))
                    }),
                ))
        });

        let card = div()
            .id("soundmoji-popout-card")
            .occlude()
            .w(px(340.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.3),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(banner)
            .child(
                div()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .size(px(40.))
                                            .rounded(px(10.))
                                            .bg(color("--background-surface-higher"))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(20.))
                                            .child(ctx.sound_emoji.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .text_size(px(15.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child(ctx.sound_name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(color("--text-muted"))
                                                    .child(dc_intl::t("D6eYmf")),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(
                                        self.with_tip(
                                            "soundmoji-vol-tip",
                                            super::tooltip::Tip::text(
                                                dc_intl::t("19lt24"),
                                                super::tooltip::Side::Top,
                                            ),
                                            div()
                                                .id("soundmoji-vol-toggle")
                                                .size(px(28.))
                                                .rounded(px(6.))
                                                .bg(color("--control-secondary-background-default"))
                                                .hover(|s| {
                                                    s.bg(color(
                                                        "--control-secondary-background-hover",
                                                    ))
                                                })
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(icon(
                                                    "sb_play",
                                                    14.,
                                                    "--interactive-icon-default",
                                                ))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(s) = this.soundmoji_popout.as_mut()
                                                    {
                                                        s.show_volume = !s.show_volume;
                                                    }
                                                    cx.notify();
                                                })),
                                            cx,
                                        ),
                                    )
                                    .child(
                                        self.with_tip(
                                            "soundmoji-info-tip",
                                            super::tooltip::Tip::text(
                                                dc_intl::t("KVbJU/"),
                                                super::tooltip::Side::Top,
                                            ),
                                            div()
                                                .id("soundmoji-info-btn")
                                                .size(px(28.))
                                                .rounded(px(6.))
                                                .bg(color("--control-secondary-background-default"))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(icon(
                                                    "circle_info",
                                                    14.,
                                                    "--interactive-icon-default",
                                                )),
                                            cx,
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(description),
                    )
                    .children(volume_panel)
                    .when_some(ctx.source_guild_name.clone(), |col, gname| {
                        col.child(
                            div()
                                .p(px(10.))
                                .rounded(px(8.))
                                .bg(color("--background-base-lower"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-muted"))
                                        .child(if ctx.is_in_guild {
                                            dc_intl::t("tGDabk")
                                        } else {
                                            dc_intl::t("rnOmOa")
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(gname),
                                ),
                        )
                    })
                    .children(cta),
            );

        Some(
            deferred(
                div()
                    .id("soundmoji-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_soundmoji_popout();
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_soundmoji(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        self.soundmoji_popout = Some(SoundmojiContext {
            sound_name: "Airhorn".to_owned(),
            sound_emoji: "📯".to_owned(),
            is_default_sound: arg == "default",
            can_send_soundmojis: arg != "beta",
            has_soundmoji_permissions: arg == "join",
            is_in_guild: arg == "same",
            is_guild_discoverable: arg != "private",
            is_sound_from_different_guild: arg != "same",
            source_guild_name: if arg == "default" {
                None
            } else {
                Some("gravecord fixture".to_owned())
            },
            volume_pct: 75,
            show_volume: true,
        });
        cx.notify();
        format!("soundmoji: {arg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soundmoji_info_covers_all_9_branches_of_456086() {
        let base = SoundmojiContext {
            sound_name: "Airhorn".to_owned(),
            sound_emoji: "📯".to_owned(),
            is_default_sound: false,
            can_send_soundmojis: true,
            has_soundmoji_permissions: true,
            is_in_guild: true,
            is_guild_discoverable: true,
            is_sound_from_different_guild: false,
            source_guild_name: Some("Guild".to_owned()),
            volume_pct: 80,
            show_volume: false,
        };
        assert_eq!(soundmoji_info(&base).0, SoundmojiButton::None);
        assert!(soundmoji_info(&base).1.contains("this server"));

        let mut c = base.clone();
        c.can_send_soundmojis = false;
        assert!(soundmoji_info(&c).1.contains("Beta"));

        let mut c = base.clone();
        c.is_default_sound = true;
        assert!(soundmoji_info(&c).1.contains("default sound"));

        let mut c = base.clone();
        c.is_in_guild = false;
        c.is_guild_discoverable = false;
        assert!(soundmoji_info(&c).1.contains("invite-only"));

        let mut c = base.clone();
        c.is_sound_from_different_guild = true;
        assert!(soundmoji_info(&c).1.contains("one of your servers"));

        let mut c = base.clone();
        c.is_in_guild = false;
        assert_eq!(soundmoji_info(&c).0, SoundmojiButton::JoinGuild);
        assert!(soundmoji_info(&c).1.contains("Join the server"));

        let mut c = base.clone();
        c.has_soundmoji_permissions = false;
        assert_eq!(soundmoji_info(&c).0, SoundmojiButton::GetNitro);
        assert!(soundmoji_info(&c).1.contains("Get Nitro"));

        let mut c = base.clone();
        c.has_soundmoji_permissions = false;
        c.is_sound_from_different_guild = true;
        assert!(soundmoji_info(&c).1.contains("Get Nitro"));

        let mut c = base;
        c.has_soundmoji_permissions = false;
        c.is_in_guild = false;
        assert!(soundmoji_info(&c).1.contains("Get Nitro and join"));
    }
}
