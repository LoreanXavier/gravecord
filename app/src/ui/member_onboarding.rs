//! The member side of server onboarding: the blocked page (module 140354, `GuildOnboardingBlockedPage`) the channel
//! route draws in place of the channel while the member has started onboarding and not finished it (961973 `$Y`,
//! called from the channel renderer `gk` of 596339 after its static routes), and its Start Onboarding button
//! (`sy` of 817818, then 197463).
//!
//! GAP: the flow Start Onboarding opens is not built. 197463 fetches the prompts (`h9`/`jx` of 817818,
//! `GET /guilds/{id}/onboarding`) and routes to `/channels/{guild}/onboarding`, the `GuildOnboardingPage` (module
//! 215764: the 3-second welcome step, the prompt pages with their options, the connections step, the rules step) whose
//! answers 669953 sends with `PUT /guilds/{id}/onboarding-responses`. Only the prompt fetch is done here.
use super::{Root, color, motion};
use crate::{net::Request, parity::Parity};
use dc_state::{Guild, Id};
use gpui::{
    AnyElement, Context, DevicePixels, FontWeight, IntoElement, ObjectFit, SvgSize, div, img,
    prelude::*, px, size,
};
use motion::Transitions as _;
use std::sync::Arc;

/// `m` of 961973: `new Date(16824888e5)`, 2023-04-26T06:00:00Z. Members who joined before it are never blocked.
const CUTOFF_MS: i64 = 1_682_488_800_000;
/// GuildMemberFlags (340837 `D`).
const COMPLETED_ONBOARDING: u64 = 2;
const STARTED_ONBOARDING: u64 = 8;
/// The fetch key the channel browser uses for the same route (`ui/channel_browser.rs`), so one answer serves both.
const PROMPTS: &str = "channel-browser:onboarding:";
/// The no-splash art is drawn blurred: a quarter of the SVG's 1000 units is all the detail it has.
const ART_RASTER: i32 = 250;

/// `o` of module 426660: the static art drawn without hardware acceleration or with animation off. With them, the
/// client plays the Rive file of 893030 instead (GAP: Rive is not played; this static art stands in).
const FALLBACK_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="1000" fill="none" preserveAspectRatio="none" viewBox="0 0 1000 1000"><g clip-path="url(#a)"><path fill="#000" d="M1000 0H0v1000h1000V0Z"/><g filter="url(#b)" opacity=".8"><path fill="#201E6E" d="M500 1922.1c349.485 0 632.8-379.26 632.8-847.1 0-467.841-283.315-847.101-632.8-847.101-349.486 0-632.8 379.26-632.8 847.101 0 467.84 283.314 847.1 632.8 847.1Z"/></g><g filter="url(#c)" opacity=".5"><path fill="#EB459E" d="M500 1428c177.283 0 321-179.98 321-402 0-222.018-143.717-402-321-402s-321 179.982-321 402c0 222.02 143.717 402 321 402Z"/></g><g filter="url(#d)"><path stroke="#5865F2" stroke-width="14.648" d="M500 1664.5c368.098 0 666.5-298.4 666.5-666.5 0-368.098-298.402-666.5-666.5-666.5S-166.5 629.902-166.5 998c0 368.1 298.402 666.5 666.5 666.5Z"/></g><g filter="url(#e)"><path fill="#5865F2" d="M500 1379c166.79 0 302-123.35 302-275.5C802 951.346 666.79 828 500 828s-302 123.346-302 275.5c0 152.15 135.21 275.5 302 275.5Z"/></g></g><defs><filter id="b" width="1753.88" height="2182.48" x="-376.941" y="-16.241" color-interpolation-filters="sRGB" filterUnits="userSpaceOnUse"><feFlood flood-opacity="0" result="BackgroundImageFix"/><feBlend in="SourceGraphic" in2="BackgroundImageFix" result="shape"/><feGaussianBlur result="effect1_foregroundBlur_1212_34965" stdDeviation="122.07"/></filter><filter id="c" width="1227.94" height="1389.94" x="-113.969" y="331.031" color-interpolation-filters="sRGB" filterUnits="userSpaceOnUse"><feFlood flood-opacity="0" result="BackgroundImageFix"/><feBlend in="SourceGraphic" in2="BackgroundImageFix" result="shape"/><feGaussianBlur result="effect1_foregroundBlur_1212_34965" stdDeviation="146.484"/></filter><filter id="d" width="1738.27" height="1738.27" x="-369.137" y="128.863" color-interpolation-filters="sRGB" filterUnits="userSpaceOnUse"><feFlood flood-opacity="0" result="BackgroundImageFix"/><feBlend in="SourceGraphic" in2="BackgroundImageFix" result="shape"/><feGaussianBlur result="effect1_foregroundBlur_1212_34965" stdDeviation="97.656"/></filter><filter id="e" width="1287.59" height="1234.59" x="-143.797" y="486.203" color-interpolation-filters="sRGB" filterUnits="userSpaceOnUse"><feFlood flood-opacity="0" result="BackgroundImageFix"/><feBlend in="SourceGraphic" in2="BackgroundImageFix" result="shape"/><feGaussianBlur result="effect1_foregroundBlur_1212_34965" stdDeviation="170.898"/></filter><clipPath id="a"><path fill="#fff" d="M0 0h1000v1000H0z"/></clipPath></defs></svg>"##;

/// The fallback art, rasterized once off the UI thread and kept by gpui's asset cache.
struct FallbackArt;

impl gpui::Asset for FallbackArt {
    type Source = ();
    type Output = Option<Arc<gpui::RenderImage>>;

    fn load(
        _source: Self::Source,
        cx: &mut gpui::App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move {
            let parsed = renderer.parse_svg(FALLBACK_SVG.as_bytes()).ok()?;
            renderer
                .render_parsed(
                    &parsed,
                    SvgSize::ExactSize(size(DevicePixels(ART_RASTER), DevicePixels(ART_RASTER))),
                )
                .ok()
        }
    }
}

/// `$Y` of module 961973: the server has `GUILD_ONBOARDING`, the member joined (on or after the cutoff), started
/// onboarding and did not complete it. A member with no join date (a lurker) is never blocked.
pub(super) fn blocked(guild: &Guild) -> bool {
    guild
        .features
        .iter()
        .any(|feature| feature == "GUILD_ONBOARDING")
        && chrono::DateTime::parse_from_rfc3339(&guild.joined_at)
            .is_ok_and(|joined| joined.timestamp_millis() >= CUTOFF_MS)
        && guild.my_flags & STARTED_ONBOARDING != 0
        && guild.my_flags & COMPLETED_ONBOARDING == 0
}

impl Root {
    /// The channel route shows the blocked page instead of the channel (`gk` of 596339 passes it the channel id, and
    /// its button needs one). The static routes (server guide, channel browser, members) are checked before it.
    pub(super) fn onboarding_blocked(&self) -> bool {
        self.channel.is_some() && self.current_guild().is_some_and(blocked)
    }

    /// Module 140354 without a splash (`fullBorderWithGradient_kgfy3D` and the 426660 art).
    /// GAP: the server's invite splash (`getGuildSplashURL`), its average-colour gradient and the `gradient_kgfy3D`
    /// layer over it are not drawn: `dc_state::Guild` keeps no `splash`.
    /// GAP: no DOM capture; how the page sizes inside the channel area (content height, full width) and the text
    /// alignment (no `text-align` rule in the CSS, so start) are not measured.
    pub(super) fn onboarding_blocked_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(guild) = self.current_guild() else {
            return div().into_any_element();
        };
        let id = guild.id;
        // `.md.hasText` of the button (65406): 38px children box (7px/15px padding) inside a 1px border, 100px wide
        // at least, `--radius-sm` corners, `body-md/medium`; `overlay-primary` colours.
        let button = div()
            .id("onboarding-blocked-start")
            .flex_none()
            .h(px(40.))
            .min_w(px(100.))
            .px(px(15.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-overlay-primary-border-default"))
            .bg(color("--control-overlay-primary-background-default"))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-overlay-primary-text-default"))
            .hover(|style| {
                style
                    .bg(color("--control-overlay-primary-background-hover"))
                    .border_color(color("--control-overlay-primary-border-hover"))
                    .text_color(color("--control-overlay-primary-text-hover"))
            })
            .active(|style| {
                style
                    .bg(color("--control-overlay-primary-background-active"))
                    .border_color(color("--control-overlay-primary-border-active"))
                    .text_color(color("--control-overlay-primary-text-active"))
            })
            .rule(motion::Rule::Button)
            .child(dc_intl::t("VVFjAC"))
            .on_click(cx.listener(move |this, _, _, cx| this.start_member_onboarding(id, cx)));
        // `explainerContainer_MLlSjX` with `transparentBackground_MLlSjX`, and `header_MLlSjX`.
        let explainer = div()
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(32.))
            .m(px(32.))
            .max_w(px(500.))
            .p(px(32.))
            .rounded(px(8.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .text_color(color("--text-overlay-light"))
                    // `heading-xxl/medium`: 32px, 500, line height 1.25.
                    .child(
                        div()
                            .text_size(px(32.))
                            .line_height(px(40.))
                            .font_weight(FontWeight(500.))
                            .child(dc_intl::t("CCmhpF")),
                    )
                    // `body-lg/normal`: 20px, 400, line height 1.2.
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(400.))
                            .child(dc_intl::t("gRAHcZ")),
                    ),
            )
            .child(button);
        // 426660's SVG: `position: absolute; bottom: 0; width: 100%`, 1000px tall, stretched (`preserveAspectRatio`
        // none).
        let art = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(1000.))
            .child(
                img(move |window: &mut gpui::Window, cx: &mut gpui::App| {
                    window.use_asset::<FallbackArt>(&(), cx).flatten().map(Ok)
                })
                .size_full()
                .object_fit(ObjectFit::Fill),
            );
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(color("--background-base-lower"))
            .child(
                // `outerWrapping_MLlSjX` (24px margin) on `main_kgfy3D` (centred column, 80px above and below, clipped)
                // with `fullBorderWithGradient_kgfy3D` (16px corners, the gradient base colour).
                div()
                    .parity("onboarding-blocked")
                    .m(px(24.))
                    .relative()
                    .overflow_hidden()
                    .rounded(px(16.))
                    .bg(color("--bg-animated-gradient-background-not-black"))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .py(px(80.))
                    .child(art)
                    .child(explainer),
            )
            .into_any_element()
    }

    /// Start Onboarding (140354 `f`): `sy` of 817818 marks the start in the client's store, then 197463 fetches the
    /// prompts (`h9`) and opens the onboarding page.
    /// GAP: `GuildOnboardingPage` (215764) is not built, so nothing opens after the fetch; flags are never changed here
    /// (COMPLETED_ONBOARDING only arrives from the server after the flow's `PUT .../onboarding-responses`).
    fn start_member_onboarding(&mut self, guild: Id, cx: &mut Context<Self>) {
        let key = format!("{PROMPTS}{guild}");
        if !self.fetched.contains_key(&key) && self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!("/guilds/{guild}/onboarding"),
            });
        }
        tracing::info!(
            guild,
            "start onboarding: the member onboarding flow (215764) is not built"
        );
        cx.notify();
    }

    /// Capture harness: `onboarding-blocked` puts the open server in the blocked state, in memory only (the feature,
    /// STARTED without COMPLETED, joined now). Nothing is sent.
    #[cfg(feature = "capture")]
    pub(crate) fn script_onboarding_blocked(&mut self) -> String {
        let now = chrono::Utc::now().to_rfc3339();
        let Some(guild) = self.guild else {
            return "onboarding-blocked: no server open".to_owned();
        };
        if let Some(known) = self
            .stores
            .guilds
            .iter_mut()
            .find(|known| known.id == guild)
        {
            if !known
                .features
                .iter()
                .any(|feature| feature == "GUILD_ONBOARDING")
            {
                known.features.push("GUILD_ONBOARDING".to_owned());
            }
            known.my_flags = (known.my_flags | STARTED_ONBOARDING) & !COMPLETED_ONBOARDING;
            known.joined_at = now;
        }
        format!(
            "onboarding-blocked: guild={guild} channel={:?} shown={}",
            self.channel,
            self.onboarding_blocked()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild(features: &[&str], flags: u64, joined_at: &str) -> Guild {
        Guild {
            features: features
                .iter()
                .map(|feature| (*feature).to_owned())
                .collect(),
            my_flags: flags,
            joined_at: joined_at.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn blocked_needs_the_feature_a_late_join_and_started_without_completed() {
        let late = "2026-11-01T00:00:00+00:00";
        assert!(blocked(&guild(&["GUILD_ONBOARDING"], 8, late)));
        // Other flag bits do not matter.
        assert!(blocked(&guild(
            &["COMMUNITY", "GUILD_ONBOARDING"],
            8 | 1 | 32,
            late
        )));
        // Completed (10 = started + completed), only completed, never started.
        assert!(!blocked(&guild(&["GUILD_ONBOARDING"], 10, late)));
        assert!(!blocked(&guild(&["GUILD_ONBOARDING"], 2, late)));
        assert!(!blocked(&guild(&["GUILD_ONBOARDING"], 0, late)));
        // No onboarding on the server.
        assert!(!blocked(&guild(&["COMMUNITY"], 8, late)));
        // No join date (a lurker) or one that does not parse.
        assert!(!blocked(&guild(&["GUILD_ONBOARDING"], 8, "")));
        assert!(!blocked(&guild(&["GUILD_ONBOARDING"], 8, "soon")));
    }

    #[test]
    fn the_cutoff_instant_is_blocked_and_a_moment_before_is_not() {
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339("2023-04-26T06:00:00Z")
                .unwrap()
                .timestamp_millis(),
            CUTOFF_MS
        );
        assert!(blocked(&guild(
            &["GUILD_ONBOARDING"],
            8,
            "2023-04-26T06:00:00Z"
        )));
        assert!(!blocked(&guild(
            &["GUILD_ONBOARDING"],
            8,
            "2023-04-26T05:59:59.999Z"
        )));
        assert!(!blocked(&guild(
            &["GUILD_ONBOARDING"],
            8,
            "2020-01-01T00:00:00+00:00"
        )));
    }

    #[test]
    fn the_fallback_art_parses() {
        let renderer = gpui::SvgRenderer::new(Arc::new(()));
        assert!(renderer.parse_svg(FALLBACK_SVG.as_bytes()).is_ok());
    }
}
