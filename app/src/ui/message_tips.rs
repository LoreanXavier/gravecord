//! Tooltips on the pieces of a message's text, which are not elements and so cannot use `with_tip`: the "(edited)"
//! mark (module 861986 wraps it in the time component of module 776579, so it has the time's tooltip and its 750 ms
//! delay: the long date of `editedTimestamp`) and a timestamp markdown (`<t:...>`, module 678473: a tooltip whose text
//! is the node's full date).

use super::Root;
use super::timestamps;
use super::tooltip::{Active, Body, Side, Tip};
use chrono::{DateTime, Local, TimeZone, Utc};
use gpui::{Bounds, Context, Pixels, SharedString};

/// The tooltip of the "(edited)" mark.
pub(super) fn edited_tip(at: DateTime<Utc>) -> Option<Tip> {
    Some(
        Tip::body(
            Body::Text(timestamps::full_time(at.with_timezone(&Local)).into()),
            Side::Top,
        )
        .after(timestamps::DELAY_MS),
    )
}

/// The tooltip of a timestamp markdown: its full date.
pub(super) fn stamp_tip(unix: i64) -> Option<Tip> {
    let at = Local.timestamp_opt(unix, 0).single()?;
    Some(Tip::body(
        Body::Text(timestamps::full_time(at).into()),
        Side::Top,
    ))
}

impl Root {
    /// The pointer moved onto a piece that has a tooltip (`Some`), or off every piece of the block `base` (`None`).
    /// Opens after the tip's delay, closes on leave, as `with_tip` does for elements.
    pub(super) fn message_tip(
        &mut self,
        base: String,
        over: Option<(usize, Tip, Bounds<Pixels>)>,
        cx: &mut Context<Self>,
    ) {
        let mine = |key: &str| key.starts_with(&base);
        let Some((piece, tip, anchor)) = over else {
            if self.tip_pending.as_ref().is_some_and(|key| mine(key)) {
                self.tip_pending = None;
            }
            if self.tip.as_ref().is_some_and(|tip| mine(tip.key())) {
                self.tip = None;
            }
            cx.notify();
            return;
        };
        let key = SharedString::from(format!("{base}-{piece}"));
        if tip.delay == 0 {
            self.tip = Some(Active::new(key, tip, anchor));
            cx.notify();
            return;
        }
        self.tip_pending = Some(key.clone());
        let wait = tip.delay;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(wait))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.tip_pending.as_ref() != Some(&key) {
                    return;
                }
                this.tip_pending = None;
                this.tip = Some(Active::new(key.clone(), tip.clone(), anchor));
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edited_mark_names_the_edit_time_after_the_times_own_delay() {
        let at = Utc.with_ymd_and_hms(2026, 11, 8, 1, 18, 0).unwrap();
        let tip = edited_tip(at).unwrap();
        assert_eq!(tip.delay, timestamps::DELAY_MS);
        match tip.body {
            Body::Text(text) => assert!(text.contains("2026"), "{text}"),
            _ => panic!("the edit time is plain text"),
        }
    }

    #[test]
    fn a_timestamp_markdown_has_a_tooltip_without_delay() {
        let tip = stamp_tip(1_794_000_000).unwrap();
        assert_eq!(tip.delay, 0);
        assert_eq!(tip.side, Side::Top);
    }
}
