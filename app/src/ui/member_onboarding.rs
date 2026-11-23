//! The member side of server onboarding: the blocked page (module 140354, `GuildOnboardingBlockedPage`) the channel
//! route draws in place of the channel while the member has started onboarding and not finished it (961973 `$Y`,
//! called from the channel renderer `gk` of 596339 after its static routes), its Start Onboarding button (`sy` of
//! 817818, then 197463), the interactive prompt flow (`GuildOnboardingPage`, module 215764, submitting via
//! `POST /guilds/{id}/onboarding-responses`, module 669953), and Guild Onboarding Connection Cards (module 528275).
use super::{Root, color, motion};
use crate::{net::Request, parity::Parity};
use dc_state::{Guild, Id};
use gpui::{
    AnyElement, Context, DevicePixels, FontWeight, Hsla, IntoElement, ObjectFit, SvgSize, div, img,
    linear_color_stop, linear_gradient, prelude::*, px, size,
};
use motion::Transitions as _;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// `m` of 961973: `new Date(16824888e5)`, 2023-04-26T06:00:00Z. Members who joined before it are never blocked.
const CUTOFF_MS: i64 = 1_682_488_800_000;
/// GuildMemberFlags (340837 `D`).
const COMPLETED_ONBOARDING: u64 = 2;
const STARTED_ONBOARDING: u64 = 8;
/// The fetch key the channel browser uses for the same route (`ui/channel_browser.rs`), so one answer serves both.
const PROMPTS: &str = "channel-browser:onboarding:";
/// Key prefix for `POST /guilds/{guild}/onboarding-responses` completion answers (`669953`).
pub(super) const SUBMIT_KEY: &str = "member-onboarding-submit:";
/// The no-splash art is drawn blurred: a quarter of the SVG's 1000 units is all the detail it has.
const ART_RASTER: i32 = 250;

/// Interactive member onboarding flow (`GuildOnboardingPage`, module 215764 / 591552 / 669953).
#[derive(Clone, Debug)]
pub(super) struct OnboardingFlow {
    pub(super) guild_id: Id,
    pub(super) return_channel_id: Option<Id>,
    pub(super) step_index: usize,
    pub(super) selected_options: HashSet<Id>,
    pub(super) prompts_seen: HashMap<Id, i64>,
    pub(super) options_seen: HashMap<Id, i64>,
    pub(super) submitting: bool,
    pub(super) error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OnboardingOption {
    id: Id,
    title: String,
    description: String,
    emoji_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OnboardingPrompt {
    id: Id,
    title: String,
    single_select: bool,
    required: bool,
    options: Vec<OnboardingOption>,
}

/// Guild Onboarding connection item (`GET /guilds/{id}/onboarding` `connections[]`, module 528275).
/// `connection_type`: `0 = APPLICATION`, `1 = PROVIDER_CONNECTED_ACCOUNT`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OnboardingConnection {
    pub connection_type: u64,
    pub application_id: Option<String>,
    pub provider_id: Option<String>,
    pub description: String,
}

pub(super) fn parse_onboarding_connections(value: &Value) -> Vec<OnboardingConnection> {
    value
        .get("connections")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|item| OnboardingConnection {
                    connection_type: item["connection_type"].as_u64().unwrap_or(1),
                    application_id: item["application_id"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned),
                    provider_id: item["provider_id"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned),
                    description: item["description"].as_str().unwrap_or_default().to_owned(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Display name & description fallback for a guild onboarding connection card (module 528275).
pub(super) fn onboarding_connection_copy(conn: &OnboardingConnection) -> (String, String) {
    let title = if conn.connection_type == 0 {
        conn.application_id
            .clone()
            .filter(|id| !id.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or_else(|| dc_intl::t("cgPbaZ").to_owned())
    } else {
        match conn.provider_id.as_deref() {
            Some("twitch") => "Twitch".to_owned(),
            Some("youtube") => "YouTube".to_owned(),
            Some("spotify") => "Spotify".to_owned(),
            Some("steam") => "Steam".to_owned(),
            Some("github") => "GitHub".to_owned(),
            Some("xbox") => "Xbox".to_owned(),
            Some("playstation") => "PlayStation Network".to_owned(),
            Some("battlenet") => "Battle.net".to_owned(),
            Some("epicgames") => "Epic Games".to_owned(),
            Some("riotgames") => "Riot Games".to_owned(),
            Some(other) if !other.is_empty() => {
                let mut chars = other.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => dc_intl::t("NzCoRx").to_owned(),
                }
            }
            _ => dc_intl::t("NzCoRx").to_owned(),
        }
    };
    let description = if !conn.description.trim().is_empty() {
        conn.description.clone()
    } else {
        match conn.provider_id.as_deref() {
            Some("twitch") => dc_intl::t("D/wRWb").to_owned(),
            Some("youtube") => dc_intl::t("TC0upt").to_owned(),
            _ => String::new(),
        }
    };
    (title, description)
}

fn guild_splash_key(guild: Id, splash: &str) -> String {
    format!("guild-splash:{guild}:{splash}")
}

fn guild_splash_url(guild: &Guild) -> Option<String> {
    let splash = guild.splash.as_deref().filter(|s| !s.is_empty())?;
    Some(format!(
        "https://cdn.discordapp.com/splashes/{}/{splash}.png?size=2048",
        guild.id
    ))
}

fn parse_id(value: &Value) -> Option<Id> {
    value
        .as_str()
        .and_then(|raw| raw.parse::<Id>().ok())
        .or_else(|| value.as_u64())
}

fn parse_onboarding_prompts(value: &Value) -> Vec<OnboardingPrompt> {
    let Some(prompts) = value.get("prompts").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for prompt in prompts {
        if !prompt
            .get("in_onboarding")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        {
            continue;
        }
        let Some(id) = prompt.get("id").and_then(parse_id) else {
            continue;
        };
        let title = prompt
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let single_select = prompt
            .get("single_select")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let required = prompt
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let options = prompt
            .get("options")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|option| {
                        let option_id = option.get("id").and_then(parse_id)?;
                        let option_title = option
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        let description = option
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        let emoji_name = option
                            .get("emoji")
                            .and_then(|e| e.get("name"))
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned);
                        Some(OnboardingOption {
                            id: option_id,
                            title: option_title,
                            description,
                            emoji_name,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(OnboardingPrompt {
            id,
            title,
            single_select,
            required,
            options,
        });
    }
    out
}

fn build_onboarding_payload(flow: &OnboardingFlow) -> Value {
    let mut responses: Vec<String> = flow
        .selected_options
        .iter()
        .map(|id| id.to_string())
        .collect();
    responses.sort();
    let prompts_seen: serde_json::Map<String, Value> = flow
        .prompts_seen
        .iter()
        .map(|(id, ts)| (id.to_string(), json!(ts)))
        .collect();
    let responses_seen: serde_json::Map<String, Value> = flow
        .options_seen
        .iter()
        .map(|(id, ts)| (id.to_string(), json!(ts)))
        .collect();
    json!({
        "onboarding_responses": responses,
        "onboarding_prompts_seen": prompts_seen,
        "onboarding_responses_seen": responses_seen,
    })
}

fn prompt_satisfied(prompt: &OnboardingPrompt, selected: &HashSet<Id>) -> bool {
    !prompt.required
        || prompt
            .options
            .iter()
            .any(|option| selected.contains(&option.id))
}

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

/// Member onboarding blocked page (`140354`) enabled now that `GuildOnboardingPage` (`215764`) and
/// `POST /guilds/{id}/onboarding-responses` (`669953`) are wired end-to-end.
const SHOW_BLOCKED_PAGE: bool = true;

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
    /// Requests the current guild's invite splash image (`getGuildSplashURL`, module 140354) when needed.
    pub(super) fn request_onboarding_splash(&mut self) {
        let Some(guild) = self.current_guild() else {
            return;
        };
        let Some(splash) = guild.splash.as_deref().filter(|s| !s.is_empty()) else {
            return;
        };
        let Some(url) = guild_splash_url(guild) else {
            return;
        };
        let key = guild_splash_key(guild.id, splash);
        self.request_image(key, url);
    }

    /// True while the interactive onboarding flow (`215764`) is open for the current server.
    pub(super) fn onboarding_flow_active(&self) -> bool {
        self.onboarding_flow
            .as_ref()
            .is_some_and(|flow| Some(flow.guild_id) == self.guild)
    }

    /// The channel route shows the blocked page instead of the channel (`gk` of 596339 passes it the channel id, and
    /// its button needs one). The static routes (server guide, channel browser, members) are checked before it.
    pub(super) fn onboarding_blocked(&self) -> bool {
        SHOW_BLOCKED_PAGE && self.channel.is_some() && self.current_guild().is_some_and(blocked)
    }

    /// Module 140354 (`GuildOnboardingBlockedPage`): renders the server's invite splash (`getGuildSplashURL`) with the
    /// average-color & `--primary-800` `gradient_kgfy3D` layers when present, or `fullBorderWithGradient_kgfy3D` and
    /// the 426660 static fallback art otherwise.
    pub(super) fn onboarding_blocked_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(guild) = self.current_guild() else {
            return div().into_any_element();
        };
        let id = guild.id;
        let splash_image = guild
            .splash
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|splash| self.images.get(&guild_splash_key(id, splash)).cloned());
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
        let backdrop: AnyElement = if let Some(splash) = splash_image {
            let mut tint: Hsla = color("--brand-500").into();
            let top_tint = Hsla { a: 0.16, ..tint };
            tint.a = 1.0;
            let mut base: Hsla = color("--primary-800").into();
            let top_base = Hsla { a: 0.5, ..base };
            base.a = 1.0;
            div()
                .absolute()
                .inset_0()
                .child(img(splash).size_full().object_fit(ObjectFit::Cover))
                .child(div().absolute().inset_0().bg(linear_gradient(
                    180.,
                    linear_color_stop(top_tint, 0.),
                    linear_color_stop(tint, 1.),
                )))
                .child(div().absolute().inset_0().bg(linear_gradient(
                    180.,
                    linear_color_stop(top_base, 0.),
                    linear_color_stop(base, 1.),
                )))
                .into_any_element()
        } else {
            // 426660's SVG: `position: absolute; bottom: 0; width: 100%`, 1000px tall, stretched (`preserveAspectRatio`
            // none).
            div()
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
                )
                .into_any_element()
        };
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
                    .child(backdrop)
                    .child(explainer),
            )
            .into_any_element()
    }

    /// Interactive member onboarding flow (`GuildOnboardingPage`, module 215764).
    pub(super) fn onboarding_flow_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(flow) = self.onboarding_flow.as_ref() else {
            return div().into_any_element();
        };
        let guild_id = flow.guild_id;
        let guild_name = self
            .stores
            .guild(guild_id)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        let key = format!("{PROMPTS}{guild_id}");
        let loaded = self.fetched.get(&key);
        let prompts = loaded.map(parse_onboarding_prompts).unwrap_or_default();
        let total = prompts.len();
        let step = flow.step_index.min(total.saturating_sub(1));

        let body: AnyElement = if loaded.is_none() {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .py(px(40.))
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-muted"))
                .child(dc_intl::t("CCmhpF"))
                .into_any_element()
        } else if let Some(prompt) = prompts.get(step) {
            let prompt_id = prompt.id;
            let single_select = prompt.single_select;
            let option_ids: Vec<Id> = prompt.options.iter().map(|o| o.id).collect();
            let can_advance = prompt_satisfied(prompt, &flow.selected_options) && !flow.submitting;
            let is_last = step + 1 >= total;
            let mut options_grid = div().flex().flex_wrap().gap(px(10.)).w_full();
            for option in &prompt.options {
                let opt_id = option.id;
                let selected = flow.selected_options.contains(&opt_id);
                let siblings = option_ids.clone();
                let is_unseen = !flow.options_seen.contains_key(&opt_id);
                // Module 164048 (`OnboardingOptionButton`, CSS `455168`): 12px radius,
                // `--opacity-blurple-8` selected fill, floating `-10px` 20px `--brand-500` check badge,
                // and floating `"NEW"` badge for unseen options.
                let card = div()
                    .id(("onboarding-option", opt_id))
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(16.))
                    .py(px(12.))
                    .rounded(px(12.))
                    .border_1()
                    .cursor_pointer()
                    .border_color(if selected {
                        color("--background-brand")
                    } else {
                        color("--border-subtle")
                    })
                    .bg(if selected {
                        color("--opacity-blurple-8")
                    } else {
                        color("--background-mod-muted")
                    })
                    .hover(|s| {
                        s.bg(if selected {
                            color("--opacity-blurple-12")
                        } else {
                            color("--background-mod-subtle")
                        })
                    })
                    .when(selected, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(-10.))
                                .right(px(-10.))
                                .size(px(20.))
                                .rounded_full()
                                .bg(color("--brand-500"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(super::icon("ob_check", 10., "--white")),
                        )
                    })
                    .when(!selected && is_unseen, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(-10.))
                                .right(px(-10.))
                                .px(px(6.))
                                .py(px(1.))
                                .rounded(px(8.))
                                .bg(color("--background-brand"))
                                .text_size(px(10.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--white"))
                                .child("NEW"),
                        )
                    })
                    .when_some(option.emoji_name.clone(), |row, emoji| {
                        row.child(div().text_size(px(18.)).child(emoji))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(option.title.clone()),
                            )
                            .when(!option.description.is_empty(), |col| {
                                col.child(
                                    div()
                                        .text_size(px(13.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(option.description.clone()),
                                )
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_onboarding_option(
                            prompt_id,
                            opt_id,
                            single_select,
                            &siblings,
                            cx,
                        );
                    }));
                options_grid = options_grid.child(card);
            }
            let primary_label = if is_last {
                dc_intl::tr("Finish")
            } else {
                dc_intl::tr("Next")
            };
            let mut footer = div()
                .flex()
                .items_center()
                .justify_between()
                .w_full()
                .pt(px(16.))
                .border_t_1()
                .border_color(color("--border-subtle"));
            let left_btn: AnyElement = if step > 0 {
                div()
                    .id("onboarding-flow-back")
                    .px(px(16.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .text_size(px(14.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .hover(|s| s.bg(color("--background-mod-subtle")))
                    .child(dc_intl::tr("Back"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(flow) = this.onboarding_flow.as_mut() {
                            flow.step_index = flow.step_index.saturating_sub(1);
                            flow.error = None;
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            } else {
                div()
                    .id("onboarding-flow-close")
                    .px(px(16.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .text_size(px(14.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-muted"))
                    .hover(|s| s.bg(color("--background-mod-subtle")))
                    .child(dc_intl::t("ETE/oC"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.onboarding_flow = None;
                        cx.notify();
                    }))
                    .into_any_element()
            };
            let right_btn = div()
                .id("onboarding-flow-next")
                .px(px(20.))
                .py(px(9.))
                .rounded(px(8.))
                .bg(if can_advance {
                    color("--brand-500")
                } else {
                    color("--background-mod-subtle")
                })
                .text_size(px(14.))
                .font_weight(FontWeight(600.))
                .text_color(if can_advance {
                    color("--white")
                } else {
                    color("--text-muted")
                })
                .when(can_advance, |btn| {
                    btn.cursor_pointer()
                        .hover(|s| s.bg(color("--brand-560")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if is_last {
                                this.submit_onboarding_responses(cx);
                            } else {
                                this.advance_onboarding_step(total, cx);
                            }
                        }))
                })
                .child(primary_label);
            footer = footer.child(left_btn).child(right_btn);

            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .w_full()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-muted"))
                                .child(format!("{guild_name} • {} / {}", step + 1, total.max(1))),
                        )
                        .when(prompt.required, |row| {
                            row.child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-feedback-warning"))
                                    .child(dc_intl::tr("Required")),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(24.))
                        .line_height(px(30.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(prompt.title.clone()),
                )
                .child(options_grid)
                .children(self.onboarding_connections_section(guild_id, cx))
                .when_some(flow.error.clone(), |col, err| {
                    col.child(
                        div()
                            .text_size(px(13.))
                            .text_color(color("--text-feedback-critical"))
                            .child(err),
                    )
                })
                .child(footer)
                .into_any_element()
        } else {
            // No in-onboarding prompts: allow finishing directly.
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(16.))
                .py(px(24.))
                .child(
                    div()
                        .text_size(px(24.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("CCmhpF")),
                )
                .child(
                    div()
                        .id("onboarding-flow-finish-empty")
                        .px(px(20.))
                        .py(px(10.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .bg(color("--brand-500"))
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(dc_intl::tr("Finish"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.submit_onboarding_responses(cx);
                        })),
                )
                .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .items_center()
            .justify_center()
            .bg(color("--background-base-lower"))
            .p(px(24.))
            .child(
                div()
                    .parity("onboarding-flow")
                    .w_full()
                    .max_w(px(640.))
                    .p(px(28.))
                    .rounded(px(12.))
                    .bg(color("--background-surface-higher"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .child(body),
            )
            .into_any_element()
    }

    fn note_onboarding_prompt_seen(&mut self) {
        let Some(flow) = self.onboarding_flow.as_mut() else {
            return;
        };
        let key = format!("{PROMPTS}{}", flow.guild_id);
        let Some(value) = self.fetched.get(&key) else {
            return;
        };
        let prompts = parse_onboarding_prompts(value);
        let Some(prompt) = prompts.get(flow.step_index) else {
            return;
        };
        let now = chrono::Utc::now().timestamp_millis();
        flow.prompts_seen.entry(prompt.id).or_insert(now);
        for option in &prompt.options {
            flow.options_seen.entry(option.id).or_insert(now);
        }
    }

    fn toggle_onboarding_option(
        &mut self,
        prompt_id: Id,
        option_id: Id,
        single_select: bool,
        siblings: &[Id],
        cx: &mut Context<Self>,
    ) {
        let Some(flow) = self.onboarding_flow.as_mut() else {
            return;
        };
        let now = chrono::Utc::now().timestamp_millis();
        flow.prompts_seen.entry(prompt_id).or_insert(now);
        for &sib in siblings {
            flow.options_seen.entry(sib).or_insert(now);
        }
        if single_select {
            let was = flow.selected_options.contains(&option_id);
            for &sib in siblings {
                flow.selected_options.remove(&sib);
            }
            if !was {
                flow.selected_options.insert(option_id);
            }
        } else if !flow.selected_options.remove(&option_id) {
            flow.selected_options.insert(option_id);
        }
        flow.error = None;
        cx.notify();
    }

    fn advance_onboarding_step(&mut self, total: usize, cx: &mut Context<Self>) {
        if let Some(flow) = self.onboarding_flow.as_mut()
            && flow.step_index + 1 < total
        {
            flow.step_index += 1;
            flow.error = None;
        }
        self.note_onboarding_prompt_seen();
        cx.notify();
    }

    /// Module 669953 (`isOnboarding = true`): `POST /guilds/{guildId}/onboarding-responses`.
    fn submit_onboarding_responses(&mut self, cx: &mut Context<Self>) {
        self.note_onboarding_prompt_seen();
        let Some(flow) = self.onboarding_flow.as_mut() else {
            return;
        };
        if flow.submitting {
            return;
        }
        flow.submitting = true;
        flow.error = None;
        let guild_id = flow.guild_id;
        let payload = build_onboarding_payload(flow);
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{SUBMIT_KEY}{guild_id}")),
            method: reqwest::Method::POST,
            route: format!("/guilds/{guild_id}/onboarding-responses"),
            body: Some(payload),
        });
        cx.notify();
    }

    /// Handles the response from `POST /guilds/{guild}/onboarding-responses`.
    pub(super) fn onboarding_submit_answered(
        &mut self,
        rest: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let Ok(guild_id) = rest.parse::<Id>() else {
            return;
        };
        let ok = body.get("ok").and_then(Value::as_bool).unwrap_or(false);
        if ok {
            if let Some(guild) = self.stores.guilds.iter_mut().find(|g| g.id == guild_id) {
                guild.my_flags |= COMPLETED_ONBOARDING;
            }
            let return_channel = self
                .onboarding_flow
                .take()
                .and_then(|flow| flow.return_channel_id);
            if let Some(channel) = return_channel {
                self.select(guild_id, Some(channel));
            }
        } else if let Some(flow) = self
            .onboarding_flow
            .as_mut()
            .filter(|flow| flow.guild_id == guild_id)
        {
            flow.submitting = false;
            flow.error = Some(
                body.get("body")
                    .and_then(|b| b.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("Unable to complete onboarding")
                    .to_owned(),
            );
        }
        cx.notify();
    }

    /// Guild Onboarding Connection Cards section (`eDVMrA` / `BozOXu` header + `528275.A` `card_yId8-H`).
    /// Shown in both `GuildOnboardingPage` (`member_onboarding.rs`) and `channel_browser.rs`.
    pub(super) fn onboarding_connections_section(
        &self,
        guild_id: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let key = format!("{PROMPTS}{guild_id}");
        let connections = parse_onboarding_connections(self.fetched.get(&key)?);
        if connections.is_empty() {
            return None;
        }
        let connected_providers: HashSet<String> = self
            .fetched
            .get("connections")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| c["type"].as_str().map(str::to_lowercase))
                    .collect()
            })
            .unwrap_or_default();

        let cards: Vec<AnyElement> = connections
            .iter()
            .enumerate()
            .map(|(i, conn)| {
                let (title, description) = onboarding_connection_copy(conn);
                let is_connected = conn
                    .provider_id
                    .as_deref()
                    .is_some_and(|p| connected_providers.contains(&p.to_lowercase()));
                let provider_for_click = conn.provider_id.clone();
                let app_for_click = conn.application_id.clone();
                let badge_letter = title
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string();
                let right: AnyElement = if is_connected {
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(super::icon("circle_check", 16., "--status-positive"))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--status-positive"))
                                .child(dc_intl::t("LV+CXH")),
                        )
                        .into_any_element()
                } else {
                    div()
                        .id(gpui::SharedString::from(format!(
                            "onboarding-conn-connect-{i}"
                        )))
                        .flex_none()
                        .h(px(32.))
                        .px(px(14.))
                        .rounded(px(6.))
                        .bg(color("--control-primary-background-default"))
                        .hover(|s| s.bg(color("--control-primary-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("S0W8Z5"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(provider) = provider_for_click.as_deref() {
                                let _ = this.requests.send(Request::Rest {
                                    key: Some(format!("connection-authorize:{provider}")),
                                    method: reqwest::Method::GET,
                                    route: format!("/connections/{provider}/authorize"),
                                    body: None,
                                });
                            } else if let Some(app_id) = app_for_click.as_deref() {
                                cx.open_url(&format!(
                                    "https://discord.com/oauth2/authorize?client_id={app_id}"
                                ));
                            }
                            cx.notify();
                        }))
                        .into_any_element()
                };
                div()
                    .id(gpui::SharedString::from(format!("onboarding-conn-{i}")))
                    .p(px(14.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(40.))
                                    .rounded(px(10.))
                                    .bg(color("--background-base-lowest"))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(badge_letter),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .truncate()
                                            .child(title),
                                    )
                                    .when(!description.is_empty(), |col| {
                                        col.child(
                                            div()
                                                .text_size(px(13.))
                                                .line_height(px(18.))
                                                .text_color(color("--text-muted"))
                                                .child(description),
                                        )
                                    }),
                            ),
                    )
                    .child(right)
                    .into_any_element()
            })
            .collect();

        Some(
            div()
                .my(px(12.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("eDVMrA")),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("BozOXu")),
                        ),
                )
                .children(cards)
                .into_any_element(),
        )
    }

    /// Capture helper: seeds onboarding connections (`528275`) and opens the member onboarding page.
    #[cfg(feature = "capture")]
    pub(crate) fn script_onboarding_connections(&mut self, cx: &mut Context<Self>) -> String {
        let Some(guild_id) = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
        else {
            return "onboarding-connections: no guild".to_owned();
        };
        let key = format!("{PROMPTS}{guild_id}");
        self.fetched.insert(
            key,
            json!({
                "prompts": [
                    {
                        "id": "9001",
                        "title": "Pick your favorite games & interests",
                        "in_onboarding": true,
                        "single_select": false,
                        "required": false,
                        "options": [
                            { "id": "9011", "title": "Valorant", "description": "Match alerts & squads" },
                            { "id": "9012", "title": "Creative Art", "description": "Weekly showcases" }
                        ]
                    }
                ],
                "connections": [
                    { "connection_type": 1, "provider_id": "twitch", "description": "" },
                    { "connection_type": 1, "provider_id": "youtube", "description": "" },
                    { "connection_type": 0, "application_id": "Riot Games Verify", "description": "Verify your Riot account for ranked channels." }
                ]
            }),
        );
        self.fetched.insert(
            "connections".to_owned(),
            json!([{ "type": "twitch", "name": "lorean" }]),
        );
        self.onboarding_flow = Some(OnboardingFlow {
            guild_id,
            return_channel_id: self.channel,
            step_index: 0,
            selected_options: HashSet::new(),
            prompts_seen: HashMap::new(),
            options_seen: HashMap::new(),
            submitting: false,
            error: None,
        });
        cx.notify();
        format!("onboarding-connections: opened for guild {guild_id}")
    }

    /// Start Onboarding (140354 `f`): `sy` of 817818 marks the start in the client's store, then 197463 fetches the
    /// prompts (`h9`) and opens `GuildOnboardingPage` (`215764`).
    fn start_member_onboarding(&mut self, guild: Id, cx: &mut Context<Self>) {
        if let Some(known) = self.stores.guilds.iter_mut().find(|g| g.id == guild) {
            known.my_flags |= STARTED_ONBOARDING;
        }
        let key = format!("{PROMPTS}{guild}");
        if !self.fetched.contains_key(&key) && self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!("/guilds/{guild}/onboarding"),
            });
        }
        self.onboarding_flow = Some(OnboardingFlow {
            guild_id: guild,
            return_channel_id: self.channel,
            step_index: 0,
            selected_options: HashSet::new(),
            prompts_seen: HashMap::new(),
            options_seen: HashMap::new(),
            submitting: false,
            error: None,
        });
        self.note_onboarding_prompt_seen();
        cx.notify();
    }

    /// Capture harness: `onboarding-blocked` puts the open server in the blocked state, in memory only (the feature,
    /// STARTED without COMPLETED, joined now). Seeds sample prompts if none are fetched so Start Onboarding can also
    /// be exercised offline. Nothing is sent.
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
        let key = format!("{PROMPTS}{guild}");
        self.fetched.entry(key).or_insert_with(|| {
            json!({
                "prompts": [
                    {
                        "id": "9001",
                        "title": "What brings you here?",
                        "single_select": false,
                        "required": true,
                        "in_onboarding": true,
                        "options": [
                            { "id": "90011", "title": "Chat & hang out", "description": "General text & voice channels", "emoji": { "name": "💬" } },
                            { "id": "90012", "title": "Updates & announcements", "description": "Release news and changelogs", "emoji": { "name": "📢" } }
                        ]
                    }
                ]
            })
        });
        format!(
            "onboarding-blocked: guild={guild} channel={:?} shown={}",
            self.channel,
            self.onboarding_blocked()
        )
    }
}

/// Module 164048 (`386784.A`): `GET /guilds/{guildId}/roles/member-counts`.
#[allow(dead_code)]
pub(crate) fn fetch_role_member_counts_request(guild_id: Id) -> crate::net::Request {
    crate::net::Request::Fetch {
        key: format!("guild-role-member-counts:{guild_id}"),
        route: format!("/guilds/{guild_id}/roles/member-counts"),
    }
}

/// Module 164048: returns the maximum member count across an onboarding option's `roleIds` (0 if empty).
#[allow(dead_code)]
pub(crate) fn max_role_member_count(role_ids: &[Id], counts: &HashMap<Id, u32>) -> u32 {
    role_ids
        .iter()
        .filter_map(|id| counts.get(id).copied())
        .max()
        .unwrap_or(0)
}

/// Module 164048 (`EgKsZA`): `"**+{memberCount}** people in common"` plain text for the 3s pill.
#[allow(dead_code)]
pub(crate) fn format_common_member_count(member_count: u32) -> String {
    dc_intl::format(
        "EgKsZA",
        &[("memberCount", dc_intl::Arg::Num(f64::from(member_count)))],
    )
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

    #[test]
    fn onboarding_prompts_parse_and_payload_matches_669953() {
        const { assert!(SHOW_BLOCKED_PAGE) };
        let raw = json!({
            "prompts": [
                {
                    "id": "101",
                    "title": "Pick roles",
                    "single_select": true,
                    "required": true,
                    "in_onboarding": true,
                    "options": [
                        { "id": "201", "title": "Rust", "description": "Systems", "emoji": { "name": "🦀" } }
                    ]
                },
                {
                    "id": "102",
                    "title": "Post-only",
                    "in_onboarding": false,
                    "options": []
                }
            ]
        });
        let prompts = parse_onboarding_prompts(&raw);
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].id, 101);
        assert!(prompts[0].single_select);
        assert!(prompts[0].required);
        assert_eq!(prompts[0].options.len(), 1);
        assert!(!prompt_satisfied(&prompts[0], &HashSet::new()));
        assert!(prompt_satisfied(&prompts[0], &HashSet::from([201])));

        let flow = OnboardingFlow {
            guild_id: 42,
            return_channel_id: Some(77),
            step_index: 0,
            selected_options: HashSet::from([201]),
            prompts_seen: HashMap::from([(101, 1_700_000_000_000)]),
            options_seen: HashMap::from([(201, 1_700_000_000_000)]),
            submitting: false,
            error: None,
        };
        let payload = build_onboarding_payload(&flow);
        assert_eq!(payload["onboarding_responses"], json!(["201"]));
        assert_eq!(
            payload["onboarding_prompts_seen"]["101"],
            json!(1_700_000_000_000_i64)
        );
        assert_eq!(
            payload["onboarding_responses_seen"]["201"],
            json!(1_700_000_000_000_i64)
        );

        let g = Guild {
            id: 42,
            splash: Some("abc123".to_owned()),
            ..Default::default()
        };
        assert_eq!(
            guild_splash_url(&g).as_deref(),
            Some("https://cdn.discordapp.com/splashes/42/abc123.png?size=2048")
        );
    }

    #[test]
    fn onboarding_option_pill_164048_member_count_and_intl() {
        assert!(dc_intl::has_key("EgKsZA"));
        let counts = HashMap::from([(10_u64, 14_u32), (11_u64, 42_u32)]);
        assert_eq!(max_role_member_count(&[10, 11], &counts), 42);
        assert_eq!(max_role_member_count(&[], &counts), 0);
        let copy = format_common_member_count(42);
        assert!(copy.contains("42"));
        assert!(matches!(
            fetch_role_member_counts_request(99),
            crate::net::Request::Fetch { route, .. } if route == "/guilds/99/roles/member-counts"
        ));
    }
}
