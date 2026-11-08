//! Invite links in messages become cards (`guildInviteContainer_fknPy8`): the link resolves through
//! `GET /invites/{code}`, and the card shows the server's banner gradient, its icon, name, online and member counts,
//! its founding month and "Go to Server" (a member) or "Join". Measured in
//! `design/dom/channel/dm-invite-embed-darker-cozy.json`.
//!
//! The banner is module 649640's `n6`: `radial-gradient(105.43% 127.05% at 50.1% 127.05%, brighten(c, 1.75) 20.65%,
//! c 85.16%)` where `c` is the icon's first palette color, or `--guild-profile-banner-background-default` without an
//! icon. gpui draws only linear gradients, so the gradient is rendered to an image once per color.

use super::{Root, color};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Id, Message};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};
use std::sync::Arc;

/// Hosts that carry invites: `discord.gg/{code}` (`INVITE_HOST`), and `/invite/{code}` on the client's own hosts.
pub(super) fn invite_codes(content: &str) -> Vec<String> {
    let mut codes = Vec::new();
    for word in content.split(|c: char| c.is_whitespace() || c == '<' || c == '>') {
        let rest = word
            .strip_prefix("https://")
            .or_else(|| word.strip_prefix("http://"))
            .unwrap_or(word);
        let code = if let Some(code) = rest.strip_prefix("discord.gg/") {
            Some(code)
        } else {
            [
                "discord.com/invite/",
                "discordapp.com/invite/",
                "canary.discord.com/invite/",
                "ptb.discord.com/invite/",
            ]
            .iter()
            .find_map(|host| rest.strip_prefix(host))
        };
        if let Some(code) = code {
            let code: String = code
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            if !code.is_empty() && !codes.contains(&code) {
                codes.push(code);
            }
        }
    }
    codes
}

/// chroma.js `brighten(amount)`: Lab lightness up by 18 per step.
pub(super) fn brighten(rgb: [u8; 3], amount: f32) -> [u8; 3] {
    let (l, a, b) = rgb_to_lab(rgb);
    lab_to_rgb(l + 18. * amount, a, b)
}

fn rgb_to_xyz_channel(value: u8) -> f32 {
    let value = f32::from(value) / 255.;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn xyz_to_rgb_channel(value: f32) -> u8 {
    let value = if value <= 0.003_130_8 {
        12.92 * value
    } else {
        1.055 * value.powf(1. / 2.4) - 0.055
    };
    (value * 255.).round().clamp(0., 255.) as u8
}

/// chroma's Lab constants: D65 white (Xn 0.950470, Zn 1.088830), t0..t3.
const XN: f32 = 0.950_47;
const YN: f32 = 1.;
const ZN: f32 = 1.088_83;
const T0: f32 = 0.137_931_04;
const T1: f32 = 0.206_896_55;
const T2: f32 = 0.128_418_55;
const T3: f32 = 0.008_856_452;

fn lab_f(t: f32) -> f32 {
    if t > T3 { t.cbrt() } else { t / T2 + T0 }
}

fn lab_f_inv(t: f32) -> f32 {
    if t > T1 { t * t * t } else { T2 * (t - T0) }
}

fn rgb_to_lab(rgb: [u8; 3]) -> (f32, f32, f32) {
    let (r, g, b) = (
        rgb_to_xyz_channel(rgb[0]),
        rgb_to_xyz_channel(rgb[1]),
        rgb_to_xyz_channel(rgb[2]),
    );
    let x = lab_f((0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / XN);
    let y = lab_f((0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b) / YN);
    let z = lab_f((0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / ZN);
    let l = 116. * y - 16.;
    (l.max(0.), 500. * (x - y), 200. * (y - z))
}

fn lab_to_rgb(l: f32, a: f32, b: f32) -> [u8; 3] {
    let y = (l + 16.) / 116.;
    let x = y + a / 500.;
    let z = y - b / 200.;
    let (x, y, z) = (XN * lab_f_inv(x), YN * lab_f_inv(y), ZN * lab_f_inv(z));
    [
        xyz_to_rgb_channel(3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z),
        xyz_to_rgb_channel(-0.969_266 * x + 1.876_010_8 * y + 0.041_556 * z),
        xyz_to_rgb_channel(0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z),
    ]
}

/// The banner as PNG bytes, `width` by `height`: the ellipse 105.43% by 127.05% of the box, centred at 50.1% across
/// and 127.05% down, `inner` up to 20.65% of the way out and `outer` from 85.16%.
pub(super) fn banner_png(base: [u8; 3], width: u32, height: u32) -> Vec<u8> {
    let inner = brighten(base, 1.75);
    let (rx, ry) = (1.0543 * width as f32, 1.2705 * height as f32);
    let (cx, cy) = (0.501 * width as f32, 1.2705 * height as f32);
    let mut pixels = image::RgbaImage::new(width, height);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let dx = (x as f32 + 0.5 - cx) / rx;
        let dy = (y as f32 + 0.5 - cy) / ry;
        let t = (dx * dx + dy * dy).sqrt();
        let mix = ((t - 0.2065) / (0.8516 - 0.2065)).clamp(0., 1.);
        let channel = |i: usize| {
            (f32::from(inner[i]) + (f32::from(base[i]) - f32::from(inner[i])) * mix).round() as u8
        };
        *pixel = image::Rgba([channel(0), channel(1), channel(2), 255]);
    }
    // The PNG encoder alone: `write_to` would build every format's encoder into this crate.
    let mut bytes = Vec::new();
    let _ = image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut bytes),
        pixels.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgba8,
    );
    bytes
}

impl Root {
    /// Codes in loaded messages that have not been asked for yet.
    pub(super) fn request_invites(&mut self) {
        let codes: Vec<String> = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .into_iter()
            .flatten()
            .flat_map(|message| invite_codes(&message.content))
            .collect();
        for code in codes {
            if self.requested_invites.insert(code.clone()) {
                let _ = self.requests.send(Request::Fetch {
                    key: format!("invite:{code}"),
                    route: format!("/invites/{code}?with_counts=true&with_expiration=true&with_permissions=true"),
                });
            }
        }
    }

    /// A resolved invite: the icon for the card and its palette.
    pub(super) fn invite_fetched(&mut self, key: &str) {
        let Some(body) = self.fetched.get(key) else {
            return;
        };
        let guild = &body["guild"];
        if let (Some(id), Some(icon)) = (guild["id"].as_str(), guild["icon"].as_str()) {
            let (id, icon) = (id.to_owned(), icon.to_owned());
            self.request_image(
                format!("guild:{id}"),
                format!("https://cdn.discordapp.com/icons/{id}/{icon}.webp?size=96"),
            );
        }
    }

    /// The banner image for a guild, made once per color.
    /// The banner color a server gets by default: its icon's main color, else `--guild-profile-banner-background-default`
    /// (module 649640 `aG`, `b2`).
    pub(super) fn guild_banner_base(&self, guild: Id) -> [u8; 3] {
        let base = self
            .images
            .get(&format!("guild:{guild}"))
            .and_then(|picture| image::load_from_memory(picture.bytes()).ok())
            .map(|decoded| crate::palette::avatar_palette(decoded.to_rgba8().as_raw())[0]);
        base.unwrap_or_else(|| {
            let token = color("--guild-profile-banner-background-default");
            [
                (token.r * 255.) as u8,
                (token.g * 255.) as u8,
                (token.b * 255.) as u8,
            ]
        })
    }

    /// Module 649640 `n6`'s gradient for `base` at a size, made once.
    pub(super) fn banner_image(&self, base: [u8; 3], width: u32, height: u32) -> Arc<gpui::Image> {
        let key = format!(
            "banner:{:02x}{:02x}{:02x}:{width}x{height}",
            base[0], base[1], base[2]
        );
        if let Some(found) = self.generated_images.borrow().get(&key) {
            return found.clone();
        }
        let made = Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            banner_png(base, width, height),
        ));
        self.generated_images.borrow_mut().insert(key, made.clone());
        made
    }

    fn invite_banner(&self, guild: Id) -> Option<Arc<gpui::Image>> {
        Some(self.banner_image(self.guild_banner_base(guild), 300, 72))
    }

    fn invite_card(&self, code: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let body = self.fetched.get(&format!("invite:{code}"))?;
        let guild = &body["guild"];
        let id: Id = guild["id"].as_str()?.parse().ok()?;
        let name = guild["name"].as_str().unwrap_or_default().to_owned();
        let online = body["approximate_presence_count"].as_u64().unwrap_or(0);
        let members = body["approximate_member_count"].as_u64().unwrap_or(0);
        let joined = self.stores.guilds.iter().any(|known| known.id == id);
        let founded =
            chrono::DateTime::from_timestamp_millis((id >> 22) as i64 + 1_420_070_400_000)
                .map(|time| {
                    time.with_timezone(&chrono::Local)
                        .format("Est. %b %Y")
                        .to_string()
                })
                .unwrap_or_default();
        let icon = self.images.get(&format!("guild:{id}")).cloned();
        let acronym = dc_state::Guild::acronym_of(&name);
        let count = |n: u64, one: &str, many: &str| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        };
        let stat = |dot: &str, text: String| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(div().size(px(8.)).rounded_full().bg(color(dot)))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child(text),
                )
        };
        let code_owned = code.to_owned();
        // Each part is erased on its own: one long builder chain here overflows the code generator.
        let avatar: AnyElement = div()
            .mt(px(-34.))
            .px(px(16.))
            .child(
                div()
                    .size(px(70.))
                    .rounded(px(20.))
                    .bg(color("--background-surface-highest"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(64.))
                            .rounded(px(18.))
                            .overflow_hidden()
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .line_height(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .when_some(icon.clone(), |this, picture| {
                                this.child(img(picture).size(px(64.)).rounded(px(18.)))
                            })
                            .when(icon.is_none(), |this| this.child(acronym)),
                    ),
            )
            .into_any_element();
        let stats: AnyElement = div()
            .h(px(18.))
            .flex()
            .gap(px(8.))
            .child(stat(
                "--icon-status-online",
                count(online, "Online", "Online"),
            ))
            .child(stat(
                "--icon-status-offline",
                count(members, "Member", "Members"),
            ))
            .into_any_element();
        let header: AnyElement = div()
            .my(px(8.))
            .px(px(16.))
            .h(px(56.))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(20.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(name),
            )
            .child(stats)
            .child(
                div()
                    .h(px(18.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-subtle"))
                    .child(founded),
            )
            .into_any_element();
        let button: AnyElement = div()
            .id(SharedString::from(format!("invite-embed-{code}")))
            .h(px(32.))
            .rounded(px(8.))
            .border_1()
            .border_color(gpui::rgba(0xffffff14))
            .bg(color("--control-connected-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--white"))
            .child(if joined { "Go to Server" } else { "Join" })
            .on_click(cx.listener(move |this, _, _, cx| {
                if joined {
                    this.select(id, None);
                } else {
                    // `acceptInvite`: POST /invites/{code}, then the server opens when it arrives.
                    let _ = this.requests.send(Request::Rest {
                        key: None,
                        method: reqwest::Method::POST,
                        route: format!("/invites/{code_owned}"),
                        body: Some(serde_json::json!({})),
                    });
                    this.pending_join = Some(id);
                }
                cx.notify();
            }))
            .into_any_element();
        let banner = self
            .invite_banner(id)
            .map(|banner| img(banner).w(px(300.)).h(px(72.)).into_any_element());
        Some(
            div()
                .parity_with_border("invite-embed", 1.)
                .w(px(302.))
                .rounded(px(16.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .overflow_hidden()
                .flex()
                .flex_col()
                .children(banner)
                .child(avatar)
                .child(header)
                .child(div().h(px(8.)))
                .child(div().px(px(16.)).pb(px(16.)).child(button))
                .into_any_element(),
        )
    }

    /// The 70px squircle-backed server picture on profile cards; the acronym measures 14px in the capture.
    pub(super) fn profile_card_avatar(
        &self,
        picture: Option<Arc<gpui::Image>>,
        acronym: String,
    ) -> AnyElement {
        div()
            .size(px(70.))
            .rounded(px(22.))
            .bg(color("--background-surface-highest"))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(64.))
                    .rounded(px(18.))
                    .overflow_hidden()
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .when_some(picture.clone(), |this, picture| {
                        this.child(img(picture).size(px(64.)).rounded(px(18.)))
                    })
                    .when(picture.is_none(), |this| this.child(acronym)),
            )
            .into_any_element()
    }

    /// Server Profile's preview (`container_Xweo-z`): the invite card's top for a joined server, counts from the store,
    /// no button; `color` is a banner preset picked on the page.
    pub(super) fn invite_preview(
        &self,
        guild: Id,
        color_override: Option<[u8; 3]>,
        name_override: Option<String>,
        body: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.invite_preview_sized(guild, color_override, name_override, body, 300., cx)
    }

    /// The card at another width (Access shows it 314 and 282 wide), its banner across it.
    pub(super) fn invite_preview_sized(
        &self,
        guild: Id,
        color_override: Option<[u8; 3]>,
        name_override: Option<String>,
        body: Option<AnyElement>,
        width: f32,
        _cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let known = self.stores.guilds.iter().find(|known| known.id == guild)?;
        // The page's edits show on the card as they are typed.
        let name = name_override.unwrap_or_else(|| known.name.clone());
        let picture = self.images.get(&format!("guild:{guild}")).cloned();
        let acronym = dc_state::Guild::acronym_of(&known.name);
        let members = self.stores.member_lists.get(&guild).map_or(1, |list| {
            list.rows
                .iter()
                .filter(|row| matches!(row, dc_state::member_list::Row::Member { .. }))
                .count()
                .max(1) as u64
        });
        let online = self
            .stores
            .member_lists
            .get(&guild)
            .and_then(|list| {
                list.groups
                    .iter()
                    .find(|(id, _)| id != "offline")
                    .map(|(_, count)| *count)
            })
            .unwrap_or(1);
        let founded =
            chrono::DateTime::from_timestamp_millis((guild >> 22) as i64 + 1_420_070_400_000)
                .map(|time| {
                    time.with_timezone(&chrono::Local)
                        .format("Est. %b %Y")
                        .to_string()
                })
                .unwrap_or_default();
        let count = |n: u64, one: &str, many: &str| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        };
        let stat = |dot: &str, text: String| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(div().size(px(8.)).rounded_full().bg(color(dot)))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child(text),
                )
        };
        let base = color_override.unwrap_or_else(|| self.guild_banner_base(guild));
        let banner = self.banner_image(base, 298, 120);
        Some(
            div()
                .w(px(width))
                .rounded(px(16.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-highest"))
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(img(banner).w(px(width - 2.)).h(px(120.)).rounded_t(px(15.)))
                .child(
                    div()
                        .mt(px(-34.))
                        .px(px(16.))
                        .child(self.profile_card_avatar(picture, acronym)),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .mb(px(8.))
                        .px(px(16.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(name),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(stat(
                                    "--icon-status-online",
                                    count(online, "Online", "Online"),
                                ))
                                .child(stat(
                                    "--icon-status-offline",
                                    count(members, "Member", "Members"),
                                )),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-subtle"))
                                .child(founded),
                        ),
                )
                // `emptyBody_Xweo-z` (8px) unless the description, games or traits fill `body_Xweo-z`.
                .child(body.unwrap_or_else(|| div().h(px(8.)).into_any_element()))
                .into_any_element(),
        )
    }

    /// The cards for a message's invite links.
    pub(super) fn invite_cards(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let cards: Vec<AnyElement> = invite_codes(&message.content)
            .iter()
            .filter_map(|code| self.invite_card(code, cx))
            .collect();
        (!cards.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(cards)
                .into_any_element()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{brighten, invite_codes};

    #[test]
    fn codes_come_from_invite_hosts_only() {
        assert_eq!(
            invite_codes(
                "join https://discord.gg/8UZeNuWZ and discord.com/invite/abc-def, https://example.com/x"
            ),
            vec!["8UZeNuWZ".to_owned(), "abc-def".to_owned()]
        );
        assert!(invite_codes("https://discord.com/channels/1/2").is_empty());
    }

    #[test]
    fn brighten_matches_the_capture() {
        // `--guild-profile-banner-background-default` (#121214) brightened 1.75 is rgb(87, 87, 89) in the capture.
        let lighter = brighten([18, 18, 20], 1.75);
        for (got, want) in lighter.iter().zip([87u8, 87, 89]) {
            assert!(got.abs_diff(want) <= 1, "{lighter:?}");
        }
    }
}
