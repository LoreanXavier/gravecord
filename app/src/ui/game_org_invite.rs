//! Game Organization Error Modal (`GameOrganizationErrorModal`, module 122393).
//!
//! Shown when a user cannot join a game organization (e.g. an in-game guild) via invite because
//! they either have no eligible game characters (`NO_CHARACTERS` -> `lXyJMS`) or hit a generic
//! join/link error (`ERROR` -> `IIR/NZ`). Directs members of the linked Discord server to `"Go to
//! Server"` (`H1wMHV`), or non-members to `"Close"` (`cpT0Cq`).

use super::{Root, color, icon};
use dc_intl::Arg;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};
use std::borrow::Cow;

/// Official error reasons (`673383.ru`, module 122393).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum GameOrgErrorReason {
    NoCharacters,
    Error,
}

/// Official organization display noun (`124077.A`: `aikK3o`/`sm5cz5`/`xXXGsq`/`nVMqjA`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum GameOrgDisplayNoun {
    #[default]
    Guild,
    Default,
}

impl GameOrgDisplayNoun {
    pub(crate) fn format(self, lowercase: bool) -> &'static str {
        let key = match (self, lowercase) {
            (Self::Guild, true) => "aikK3o",
            (Self::Guild, false) => "sm5cz5",
            (Self::Default, true) => "xXXGsq",
            (Self::Default, false) => "nVMqjA",
        };
        if dc_intl::has_key(key) {
            dc_intl::t(key)
        } else if lowercase {
            "guild"
        } else {
            "Guild"
        }
    }
}

/// Resolved game-organization invite snapshot (`580583.A.getInvite(code)`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedGameOrgInvite {
    pub code: String,
    pub guild_id: Id,
    pub guild_name: String,
    pub display_noun: GameOrgDisplayNoun,
}

/// Open state for `GameOrganizationErrorModal` (module 122393).
#[derive(Clone, Debug)]
pub(crate) struct GameOrgErrorModalState {
    pub invite: ResolvedGameOrgInvite,
    pub reason: GameOrgErrorReason,
}

/// Module 122393: builds `(title, subtitle)` from `DinVUQ` + `lXyJMS`/`IIR/NZ`.
pub(crate) fn game_org_error_copy(
    invite: &ResolvedGameOrgInvite,
    reason: GameOrgErrorReason,
) -> (String, String) {
    let noun = invite.display_noun.format(true);
    let title = if dc_intl::has_key("DinVUQ") {
        dc_intl::format("DinVUQ", &[("noun", Arg::Str(Cow::Borrowed(noun)))])
    } else {
        format!("You can't join this {noun}")
    };
    let sub_key = match reason {
        GameOrgErrorReason::NoCharacters => "lXyJMS",
        GameOrgErrorReason::Error => "IIR/NZ",
    };
    let subtitle = if dc_intl::has_key(sub_key) {
        dc_intl::format(
            sub_key,
            &[
                ("noun", Arg::Str(Cow::Borrowed(noun))),
                ("guildName", Arg::Str(Cow::Borrowed(&invite.guild_name))),
            ],
        )
    } else if reason == GameOrgErrorReason::NoCharacters {
        format!(
            "You don't have any eligible characters to join this {noun}. Check out the server linked to this {noun}, {}, instead.",
            invite.guild_name
        )
    } else {
        format!(
            "Unfortunately something went wrong. Check out the server linked to this {noun}, {}, instead.",
            invite.guild_name
        )
    };
    (title, subtitle)
}

impl Root {
    /// Renders `GameOrganizationErrorModal` (module 122393) when open.
    pub(super) fn game_org_error_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.game_org_error_modal.as_ref()?;
        let (title, subtitle) = game_org_error_copy(&state.invite, state.reason);
        let guild_id = state.invite.guild_id;
        let in_linked_guild = self.stores.guild(guild_id).is_some();
        let action_label = if in_linked_guild {
            if dc_intl::has_key("H1wMHV") {
                dc_intl::t("H1wMHV")
            } else {
                "Go to Server"
            }
        } else {
            dc_intl::t("cpT0Cq")
        };

        Some(
            deferred(
                div()
                    .id("game-org-error-backdrop")
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
                            this.game_org_error_modal = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id("game-org-error-modal")
                            .w(px(440.))
                            .max_w(px(440.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(
                                div()
                                    .px(px(24.))
                                    .pt(px(24.))
                                    .pb(px(16.))
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
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .text_size(px(20.))
                                                    .line_height(px(26.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(title),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .line_height(px(20.))
                                                    .text_color(color("--text-muted"))
                                                    .child(subtitle),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("game-org-error-close-x")
                                            .size(px(28.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.game_org_error_modal = None;
                                                cx.notify();
                                            }))
                                            .child(icon("modal_close", 16., "--icon-subtle")),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(24.))
                                    .py(px(16.))
                                    .bg(color("--background-base-lower"))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .child(
                                        div()
                                            .id("game-org-error-action")
                                            .h(px(38.))
                                            .px(px(18.))
                                            .rounded(px(8.))
                                            .bg(color(if in_linked_guild {
                                                "--control-primary-background-default"
                                            } else {
                                                "--control-secondary-background-default"
                                            }))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color(if in_linked_guild {
                                                "--white"
                                            } else {
                                                "--control-secondary-text-default"
                                            }))
                                            .child(action_label)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.game_org_error_modal = None;
                                                if in_linked_guild {
                                                    this.select(guild_id, None);
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_game_org_error(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "close" {
            self.game_org_error_modal = None;
            cx.notify();
            return "game-org-error: closed".to_owned();
        }
        let reason = if arg.trim() == "error" {
            GameOrgErrorReason::Error
        } else {
            GameOrgErrorReason::NoCharacters
        };
        let (guild_id, guild_name) = self
            .stores
            .guilds
            .first()
            .map(|g| (g.id, g.name.clone()))
            .unwrap_or((999, "Azeroth Vanguard".to_owned()));
        self.game_org_error_modal = Some(GameOrgErrorModalState {
            invite: ResolvedGameOrgInvite {
                code: "guild-invite-demo".to_owned(),
                guild_id: if arg.trim() == "not-member" {
                    999_999_999
                } else {
                    guild_id
                },
                guild_name,
                display_noun: GameOrgDisplayNoun::Guild,
            },
            reason,
        });
        cx.notify();
        format!("game-org-error: opened ({reason:?})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_org_error_122393_copy_and_intl() {
        assert!(dc_intl::has_key("cpT0Cq"));
        let inv = ResolvedGameOrgInvite {
            code: "abc".into(),
            guild_id: 42,
            guild_name: "Ironforge".into(),
            display_noun: GameOrgDisplayNoun::Guild,
        };
        let (t1, s1) = game_org_error_copy(&inv, GameOrgErrorReason::NoCharacters);
        assert!(t1.contains("guild"));
        assert!(s1.contains("Ironforge") && s1.contains("characters"));
        let (_, s2) = game_org_error_copy(&inv, GameOrgErrorReason::Error);
        assert!(s2.contains("Ironforge") && s2.contains("wrong"));
        assert_eq!(GameOrgDisplayNoun::Guild.format(true), "guild");
        assert_eq!(GameOrgDisplayNoun::Default.format(false), "Guild");
    }
}
