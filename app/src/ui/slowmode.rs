//! Composer notice: modules 620014 and 960850; `cooldownWrapper_bbCgC2` in the channel DOM.
use super::{Root, color, icon, tooltip};
use crate::parity::Parity;
use dc_state::{Id, slowmode::Kind};
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px};
use std::time::{Duration, Instant};

fn cooldown_label(millis: u64, bypass: bool) -> String {
    if bypass {
        return "Slowmode Immune".to_owned();
    }
    if millis == 0 {
        return "Slowmode is enabled.".to_owned();
    }
    let seconds = millis / 1000;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            (seconds / 60) % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

fn interval_text(seconds: u64) -> String {
    let unit =
        |count: u64, name: &str| format!("{count} {name}{}", if count == 1 { "" } else { "s" });
    let duration = if seconds < 60 {
        format!("{}.", unit(seconds, "second"))
    } else if seconds < 3600 {
        format!(
            "{} {}",
            unit(seconds / 60, "minute"),
            if seconds.is_multiple_of(60) {
                String::new()
            } else {
                unit(seconds % 60, "second")
            }
        )
    } else {
        let minutes = (seconds % 3600) / 60;
        format!(
            "{} {} {}",
            unit(seconds / 3600, "hour"),
            if minutes == 0 {
                String::new()
            } else {
                unit(minutes, "minute")
            },
            if seconds.is_multiple_of(60) {
                String::new()
            } else {
                unit(seconds % 60, "second")
            }
        )
    };
    format!("Slowmode is enabled. Members can send one message every {duration}")
}

impl Root {
    pub(super) fn restore_slowmode_draft(&mut self, cx: &mut Context<Self>) {
        let Some(channel) = self.channel else {
            return;
        };
        if !self.composer.read(cx).text().is_empty()
            || !self.pending_files.is_empty()
            || self.replying.is_some()
        {
            return;
        }
        let Some(draft) = self
            .slowmode_rejected
            .get_mut(&channel)
            .and_then(|queue| queue.pop_front())
        else {
            return;
        };
        self.composer
            .update(cx, |composer, _| composer.set_text(&draft.content));
        self.pending_files = draft
            .files
            .into_iter()
            .filter_map(super::uploads::Pending::from_upload)
            .collect();
        if let Some((_, message, ping)) = draft.reply {
            self.replying = self
                .stores
                .messages
                .get(&channel)
                .and_then(|messages| messages.iter().find(|known| known.id == message))
                .cloned();
            self.reply_ping = ping;
        }
    }

    pub(super) fn slowmode_channel(&self, channel: Id) -> Option<(u64, bool)> {
        let me = self.stores.me()?;
        self.stores.guilds.iter().find_map(|guild| {
            let target = guild.channels.iter().find(|known| known.id == channel)?;
            Some((
                target.rate_limit_per_user,
                dc_state::permissions::can_in_channel(
                    guild,
                    target,
                    me,
                    dc_state::permissions::BYPASS_SLOWMODE,
                ),
            ))
        })
    }

    pub(super) fn set_slowmode(
        &mut self,
        channel: Id,
        kind: Kind,
        millis: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let Some((rate, bypass)) = self.slowmode_channel(channel) else {
            return;
        };
        let now = Instant::now();
        match millis {
            Some(millis) => self.slowmode.set(channel, kind, rate, bypass, millis, now),
            None => self.slowmode.reset(channel, kind, rate, bypass, now),
        }
        if !self.slowmode_ticking
            && let Some(mut delay) = self.slowmode.next_tick_delay(now)
        {
            self.slowmode_ticking = true;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(delay).await;
                    let next = this
                        .update(cx, |this, cx| {
                            let now = Instant::now();
                            this.slowmode.tick(now);
                            let next = this.slowmode.next_tick_delay(now);
                            this.slowmode_ticking = next.is_some();
                            cx.notify();
                            next
                        })
                        .ok()
                        .flatten();
                    let Some(next) = next else {
                        break;
                    };
                    delay = next;
                }
            })
            .detach();
        }
        cx.notify();
    }

    pub(super) fn hold_slowmode(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(channel) = self.channel else {
            return false;
        };
        let Some((rate, bypass)) = self.slowmode_channel(channel) else {
            return false;
        };
        if !self
            .slowmode
            .is_channel_on_cooldown(channel, Kind::SendMessage, rate, bypass)
        {
            return false;
        }
        self.composer
            .update(cx, |composer, _| composer.set_text(text));
        self.slowmode_emphasis = Some(Instant::now());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        cx.notify();
        true
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_slowmode(&mut self, millis: u64, cx: &mut Context<Self>) {
        if let Some(channel) = self.channel {
            self.set_slowmode(channel, Kind::SendMessage, Some(millis), cx);
        }
    }

    /// Capture-only permission view, confined to the fixture. It never writes the guild or account.
    #[cfg(feature = "capture")]
    pub(crate) fn script_slowmode_fixture(&mut self, millis: u64, cx: &mut Context<Self>) {
        const FIXTURE: Id = 1568692564637524109;
        if self.guild != Some(FIXTURE) {
            return;
        }
        let Some(channel) = self.channel else {
            return;
        };
        let Some(guild) = self
            .stores
            .guilds
            .iter_mut()
            .find(|guild| guild.id == FIXTURE)
        else {
            return;
        };
        let Some(target) = guild
            .channels
            .iter_mut()
            .find(|target| target.id == channel && target.kind == dc_state::ChannelKind::Text)
        else {
            return;
        };
        target.rate_limit_per_user = 5;
        guild.owner_id = None;
        guild.role_permissions.values_mut().for_each(|bits| {
            *bits &=
                !(dc_state::permissions::ADMINISTRATOR | dc_state::permissions::BYPASS_SLOWMODE)
        });
        self.set_slowmode(channel, Kind::SendMessage, Some(millis), cx);
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_slowmode_rejection(&mut self, cx: &mut Context<Self>) {
        if self.guild != Some(1568692564637524109) {
            return;
        }
        if let Some(channel) = self.channel {
            self.update(
                crate::net::Update::SlowmodeRejected {
                    channel,
                    draft: crate::net::RejectedDraft {
                        content: "Recovered slowmode draft".to_owned(),
                        files: Vec::new(),
                        reply: None,
                    },
                    reason: None,
                },
                cx,
            );
        }
    }

    pub(super) fn slowmode_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.current_channel()?;
        if channel.rate_limit_per_user == 0 {
            return None;
        }
        let bypass = self
            .current_guild()
            .zip(self.stores.me())
            .is_some_and(|(guild, me)| {
                dc_state::permissions::can_in_channel(
                    guild,
                    channel,
                    me,
                    dc_state::permissions::BYPASS_SLOWMODE,
                )
            });
        let label = cooldown_label(
            self.slowmode
                .get_slowmode_cooldown_guess(channel.id, Kind::SendMessage),
            bypass,
        );
        let foreground = if self
            .slowmode_emphasis
            .is_some_and(|at| at.elapsed() < Duration::from_secs(1))
        {
            "--text-feedback-critical"
        } else {
            "--text-muted"
        };
        let notice = div()
            .parity("slowmode-notice")
            .py(px(3.))
            .pl(px(8.))
            .pr(px(16.))
            .flex()
            .items_center()
            .child(
                div()
                    .ml(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .font_features(gpui::FontFeatures(std::sync::Arc::new(vec![
                        ("kern".to_owned(), 1),
                        ("tnum".to_owned(), 1),
                    ])))
                    .text_color(color(foreground))
                    .child(icon("slowmode", 12., foreground))
                    .child(label),
            );
        Some(
            div()
                .absolute()
                .right_0()
                .top(px(-23.))
                .child(self.with_tip(
                    "slowmode",
                    tooltip::Tip::body(
                        tooltip::Body::Text(interval_text(channel.rate_limit_per_user).into()),
                        tooltip::Side::Top,
                    ),
                    notice,
                    cx,
                ))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn countdown_formats_remaining_seconds_and_permission_override() {
        assert_eq!(cooldown_label(0, false), "Slowmode is enabled.");
        assert_eq!(cooldown_label(100, false), "0:00");
        assert_eq!(cooldown_label(61000, false), "1:01");
        assert_eq!(cooldown_label(3661000, false), "1:01:01");
        assert_eq!(cooldown_label(61000, true), "Slowmode Immune");
    }
    #[test]
    fn interval_notice_uses_the_pinned_plural_forms() {
        assert_eq!(
            interval_text(1),
            "Slowmode is enabled. Members can send one message every 1 second."
        );
        assert_eq!(
            interval_text(5),
            "Slowmode is enabled. Members can send one message every 5 seconds."
        );
        assert_eq!(
            interval_text(60),
            "Slowmode is enabled. Members can send one message every 1 minute "
        );
        assert_eq!(
            interval_text(61),
            "Slowmode is enabled. Members can send one message every 1 minute 1 second"
        );
        assert_eq!(
            interval_text(3600),
            "Slowmode is enabled. Members can send one message every 1 hour  "
        );
        assert_eq!(
            interval_text(3661),
            "Slowmode is enabled. Members can send one message every 1 hour 1 minute 1 second"
        );
    }
}
