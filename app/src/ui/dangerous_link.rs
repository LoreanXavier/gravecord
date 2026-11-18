//! "Dangerous Website Ahead" (module 443198): a link to a known abusive domain never opens. `Y` of module 889558 asks
//! `isBlockedDomain` (module 306173, libdiscore's BlockedDomainsStore) after the app's own routes and before any trust,
//! and shows this modal with only Go Back.
//!
//! The list is `https://cdn.discordapp.com/bad-domains/updated_hashes.json` (`q` of module 596339 and the libdiscore
//! strings): the SHA-256 of each domain, hex. Asked in the pinned client, `isBlockedDomain` lowercases the host, drops
//! the port, and answers the listed domain for the host or any of its parent domains (`free.discordgift.site` ->
//! `discordgift.site`), `null` otherwise. The list is read once a session; libdiscore's delta updates by
//! `current_revision.txt` are not ported.

use super::channel_status::{HEADER_WIDTH, MODAL_WIDTH, header_gradient};
use super::{Root, color, icon, links};
use crate::parity::Parity;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, StyledText, deferred, div, img, prelude::*, px,
    rgba,
};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::RwLock;

/// The key the list comes back under.
pub(super) const BAD_DOMAINS_KEY: &str = "bad-domains";
pub(super) const BAD_DOMAINS_URL: &str =
    "https://cdn.discordapp.com/bad-domains/updated_hashes.json";
/// Module 117816, the modal's graphic (288 by 162, at most 15vh: 192 by 108 in a 720 tall window).
const GRAPHIC: &str = "https://cdn.discordapp.com/assets/content/971d3a140aa11a3830346a83266ea28b2f4e586e9b4f02eed4315538f6a9ebbe.svg";

static HASHES: RwLock<Option<HashSet<[u8; 32]>>> = RwLock::new(None);
/// The list was asked for this session.
static ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The list as the CDN serves it: an array of hex digests.
pub(super) fn parse_hashes(body: &serde_json::Value) -> HashSet<[u8; 32]> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .filter_map(|hex| {
            let mut out = [0u8; 32];
            (hex.len() == 64).then_some(())?;
            for (index, byte) in out.iter_mut().enumerate() {
                *byte = u8::from_str_radix(hex.get(index * 2..index * 2 + 2)?, 16).ok()?;
            }
            Some(out)
        })
        .collect()
}

/// `isBlockedDomain`: the listed domain the URL's host is, or sits under.
pub(super) fn blocked_domain_in(hashes: &HashSet<[u8; 32]>, url: &str) -> Option<String> {
    let host = links::hostname(url);
    if host.is_empty() {
        return None;
    }
    let labels: Vec<&str> = host.split('.').collect();
    (0..labels.len()).find_map(|start| {
        let domain = labels[start..].join(".");
        let digest: [u8; 32] = Sha256::digest(domain.as_bytes()).into();
        hashes.contains(&digest).then_some(domain)
    })
}

/// The check against the list this session read; nothing is blocked before it arrived.
pub(super) fn blocked_domain(url: &str) -> Option<String> {
    let hashes = HASHES.read().ok()?;
    blocked_domain_in(hashes.as_ref()?, url)
}

impl Root {
    /// `startFetchingBlockedDomains`: the list is asked for once.
    pub(super) fn request_blocked_domains(&mut self) {
        if ASKED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let _ = self.requests.send(crate::net::Request::FetchPublicJson {
            key: BAD_DOMAINS_KEY.to_owned(),
            url: BAD_DOMAINS_URL.to_owned(),
        });
    }

    /// The list arrived.
    pub(super) fn blocked_domains_fetched(&mut self, body: &serde_json::Value) {
        let hashes = parse_hashes(body);
        if hashes.is_empty() {
            return;
        }
        if let Ok(mut slot) = HASHES.write() {
            *slot = Some(hashes);
        }
    }

    /// Module 443198 on the design-system modal of 772707 (`size md`, blue gradient header): the graphic, "Dangerous
    /// Website Ahead", the warning with its bold phrase, the address in `linkCalloutContainer_Wv2eLj` (protocol and
    /// path muted, host semibold) and Go Back, primary and full width. Header geometry as the voice status modal's
    /// capture measured it, the subtitle on as many lines as it takes.
    pub(super) fn dangerous_link_modal(&self, url: &str, cx: &mut Context<Self>) -> AnyElement {
        let parts = links::split(url);
        let art = self.images.get(GRAPHIC).cloned();
        if art.is_none() {
            self.want_image(GRAPHIC.to_owned(), GRAPHIC.to_owned(), cx);
        }
        let gradient = header_gradient(
            color("--expressive-gradient-blue-start"),
            color("--expressive-gradient-blue-end"),
        );
        // `jnHyYU`: the `$b` part is "a known abusive domain" (in another language it is drawn without the bold).
        let bold = "a known abusive domain";
        let subtitle = dc_intl::t("jnHyYU").to_owned();
        let bold_at = subtitle.find(bold);
        let subtitle_text = match bold_at {
            Some(at) => StyledText::new(subtitle.clone()).with_highlights([(
                at..at + bold.len(),
                gpui::HighlightStyle {
                    font_weight: Some(FontWeight(600.)),
                    ..Default::default()
                },
            )]),
            None => StyledText::new(subtitle.clone()),
        };
        let header = div()
            .relative()
            .flex_none()
            .w(px(HEADER_WIDTH as f32))
            .pt(px(28.))
            .pb(px(16.))
            .px(px(32.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                img(gradient)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .object_fit(gpui::ObjectFit::Fill),
            )
            .child(
                div()
                    .h(px(108.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(art.map(|art| img(art).w(px(192.)).h(px(108.)))),
            )
            .child(
                div()
                    .mt(px(24.))
                    .w_full()
                    .flex()
                    .justify_center()
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::t("2B3wj8")),
            )
            .child(
                div()
                    .mt(px(8.))
                    .w_full()
                    .text_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-subtle"))
                    .child(subtitle_text),
            );
        let callout = div()
            .mx(px(32.))
            .p(px(16.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .max_h(px(144.))
            .overflow_hidden()
            .flex()
            .flex_wrap()
            .text_size(px(16.))
            .line_height(px(20.))
            .child(
                div()
                    .text_color(color("--text-muted"))
                    .child(format!("{}//", parts.protocol)),
            )
            .child(
                div()
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-default"))
                    .child(parts.host.clone()),
            )
            .child(div().text_color(color("--text-muted")).child(parts.rest));
        let footer = div().flex_none().px(px(32.)).py(px(16.)).flex().child(
            div()
                .id("dangerous-go-back")
                .flex_1()
                .h(px(40.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--control-primary-border-default"))
                .bg(color("--control-primary-background-default"))
                .hover(|style| style.bg(color("--control-primary-background-hover")))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--white"))
                .child(dc_intl::t("/g10LC"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.leaving = None;
                    cx.notify();
                })),
        );
        let pose = crate::ui::motion::modal_pose("dangerous-link");
        let panel = div()
            .parity_with_border("dangerous-link-modal", 1.)
            .id("dangerous-link-modal")
            .opacity(pose.opacity)
            .paint_scale(pose.scale, gpui::point(0.5, 0.5))
            .occlude()
            .relative()
            .w(px(MODAL_WIDTH))
            .pt(px(8.))
            .pb(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(header)
            .child(div().flex_none().h(px(16.)))
            .child(callout)
            .child(div().flex_none().h(px(16.)))
            .child(footer)
            .child(
                div()
                    .id("dangerous-link-close")
                    .absolute()
                    .top(px(8.))
                    .right(px(8.))
                    .size(px(38.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("modal_close", 20., "--text-strong"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.leaving = None;
                        cx.notify();
                    })),
            );
        deferred(
            div()
                .id("dangerous-link-layer")
                .occlude()
                .absolute()
                .inset_0()
                .bg(pose.dim(rgba(0x000000b8)))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.leaving = None;
                    cx.notify();
                }))
                .child(panel),
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(domains: &[&str]) -> HashSet<[u8; 32]> {
        let hex: Vec<serde_json::Value> = domains
            .iter()
            .map(|domain| {
                let digest = Sha256::digest(domain.as_bytes());
                serde_json::Value::String(digest.iter().map(|byte| format!("{byte:02x}")).collect())
            })
            .collect();
        parse_hashes(&serde_json::Value::Array(hex))
    }

    #[test]
    fn a_listed_domain_and_its_subdomains_are_blocked_by_the_listed_name() {
        // As the pinned client answered for these addresses.
        let hashes = list(&["discordgift.site", "discord-gift.com"]);
        let blocked = |url: &str| blocked_domain_in(&hashes, url);
        assert_eq!(
            blocked("https://discordgift.site/x").as_deref(),
            Some("discordgift.site")
        );
        assert_eq!(
            blocked("https://free.discordgift.site/x").as_deref(),
            Some("discordgift.site")
        );
        assert_eq!(
            blocked("https://a.b.discordgift.site").as_deref(),
            Some("discordgift.site")
        );
        assert_eq!(
            blocked("https://www.discord-gift.com").as_deref(),
            Some("discord-gift.com")
        );
        assert_eq!(
            blocked("https://DiscordGift.SITE/").as_deref(),
            Some("discordgift.site")
        );
        assert_eq!(
            blocked("http://discordgift.site:8080/").as_deref(),
            Some("discordgift.site")
        );
        assert_eq!(blocked("https://discordgift.site.example.com"), None);
        assert_eq!(blocked("https://example.com"), None);
    }

    #[test]
    fn the_list_reads_hex_digests_and_skips_anything_else() {
        let body = serde_json::json!([
            "00002ab0c0d8389e287380567293a8662bd6400d65ed956711da2d4395c46179",
            "short",
            12,
        ]);
        assert_eq!(parse_hashes(&body).len(), 1);
    }
}
