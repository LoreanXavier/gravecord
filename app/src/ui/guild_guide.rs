//! Member-facing Server Guide: 452849, 616740, 701785 and 65995.
use super::{Root, avatar_source, color, icon, server_onboarding::Home};
use crate::{net::Request, parity::Parity};
use dc_state::{Guild, Id};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};
use std::sync::Arc;

const HOME: &str = "guild-guide:home:";
const ACTIONS: &str = "guild-guide:actions:";

#[derive(Clone, Hash, PartialEq, Eq)]
struct BannerSource {
    image: Arc<gpui::Image>,
    placeholder: bool,
}

struct BannerImage;

fn banner_crop(width: u32, height: u32, placeholder: bool) -> (u32, u32, u32, u32) {
    let ratio = if placeholder { 0.12 } else { 0.25 };
    let cropped_height = (width as f64 * ratio).round().max(1.) as u32;
    let (width_out, height_out) = if cropped_height <= height {
        (width, cropped_height)
    } else {
        ((height as f64 / ratio).round().max(1.) as u32, height)
    };
    (
        (width - width_out) / 2,
        (height - height_out) / 2,
        width_out,
        height_out,
    )
}

impl gpui::Asset for BannerImage {
    type Source = BannerSource;
    type Output = Option<Arc<gpui::RenderImage>>;

    fn load(
        source: Self::Source,
        cx: &mut gpui::App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move {
            let decoded = source.image.to_image_data(renderer).ok()?;
            let size = decoded.size(0);
            let width = u32::from(size.width);
            let height = u32::from(size.height);
            if width == 0 || height == 0 {
                return None;
            }
            let pixels = image::RgbaImage::from_raw(width, height, decoded.as_bytes(0)?.to_vec())?;
            let (x, y, width, height) = banner_crop(width, height, source.placeholder);
            let cropped = image::imageops::crop_imm(&pixels, x, y, width, height).to_image();
            Some(Arc::new(gpui::RenderImage::new(vec![image::Frame::new(
                cropped,
            )])))
        }
    }
}

fn new_member(guild: &Guild, now: i64) -> bool {
    guild.my_flags & 64 == 0
        && chrono::DateTime::parse_from_rfc3339(&guild.joined_at)
            .is_ok_and(|joined| now - joined.timestamp_millis() < 7 * 24 * 60 * 60 * 1000)
}

fn enabled(guild: &Guild) -> bool {
    ["COMMUNITY", "GUILD_SERVER_GUIDE", "GUILD_ONBOARDING"]
        .iter()
        .all(|feature| guild.features.iter().any(|known| known == feature))
}

fn welcome_text(template: &str, name: &str) -> (String, Vec<std::ops::Range<usize>>) {
    let normalized = template
        .split([' ', '\n', '\r', '\t', '\u{c}'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (index, part) in normalized.split("[@username]").enumerate() {
        if index > 0 {
            let start = text.len();
            text.push('@');
            text.push_str(name);
            highlights.push(start..text.len());
        }
        text.push_str(part);
    }
    (text, highlights)
}

impl Root {
    fn guide_home(&self) -> Option<Home> {
        let guild = self.current_guild()?;
        let value = self.fetched.get(&format!("{HOME}{}", guild.id))?;
        if value.is_null() {
            return None;
        }
        Some(Home::from_wire(value, &|id| {
            id.parse::<Id>()
                .ok()
                .is_some_and(|id| guild.channels.iter().any(|channel| channel.id == id))
        }))
    }

    pub(super) fn has_guild_guide(&self) -> bool {
        self.current_guild().is_some_and(|guild| {
            enabled(guild)
                && (new_member(guild, chrono::Utc::now().timestamp_millis())
                    || guild
                        .channels
                        .iter()
                        .any(|channel| channel.flags & 128 != 0))
        })
    }

    pub(super) fn prepare_guild_guide(&mut self) {
        let Some(guild) = self.current_guild().filter(|guild| enabled(guild)) else {
            return;
        };
        let id = guild.id;
        let flags = guild.my_flags;
        let guild_icon = guild.icon.clone();
        let header = guild.home_header.clone();
        for (prefix, suffix) in [
            (HOME, "new-member-welcome"),
            (ACTIONS, "new-member-actions"),
        ] {
            if prefix == ACTIONS && flags & 32 == 0 {
                continue;
            }
            let key = format!("{prefix}{id}");
            if self.requested.insert(key.clone()) {
                let _ = self.requests.send(Request::Fetch {
                    key,
                    route: format!("/guilds/{id}/{suffix}"),
                });
            }
        }
        if let Some(hash) = guild_icon {
            self.request_image(
                format!("guide-icon:{id}"),
                format!("https://cdn.discordapp.com/icons/{id}/{hash}.webp?size=100"),
            );
        }
        let art = if let Some(hash) = header {
            format!("https://cdn.discordapp.com/home-headers/{id}/{hash}.png?size=1280")
        } else {
            let hash = if super::theme() == dc_tokens::Theme::Light {
                "3655f5d8659277b4"
            } else {
                "36eaea47ad7cb391"
            };
            format!("https://discord.com/assets/{hash}.svg")
        };
        self.request_image(art.clone(), art);
        if let Some(home) = self.guide_home() {
            let authors: Vec<Id> = home
                .welcome
                .as_ref()
                .into_iter()
                .flat_map(|welcome| &welcome.author_ids)
                .filter_map(|id| id.parse().ok())
                .collect();
            if !authors.is_empty() && self.requested.insert(format!("guide-authors:{id}")) {
                let _ = self.requests.send(Request::GuildMembers {
                    guilds: vec![id],
                    query: String::new(),
                    limit: 0,
                    user_ids: authors.clone(),
                });
            }
            for author in authors {
                if let Some(user) = self.stores.users.get(&author) {
                    let (key, url) = avatar_source(user);
                    self.request_image(key, url);
                }
            }
        }
    }

    pub(super) fn open_guild_guide(&mut self, cx: &mut Context<Self>) {
        if !self.has_guild_guide() {
            return;
        }
        let from = self.route();
        if !self.guild_guide_open {
            self.guild_guide_scroll.set_offset(Default::default());
        }
        let mut to = from;
        to.channel = None;
        to.guild_guide = true;
        self.history.visit(from, to);
        self.channel = None;
        self.member_view = None;
        self.guild_guide_open = true;
        cx.notify();
    }

    pub(super) fn guild_guide_rows(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.has_guild_guide() {
            return None;
        }
        let guild = self.current_guild()?;
        let home = self.guide_home();
        let tasks = home.as_ref().and_then(|home| home.tasks.as_ref());
        let total = tasks.map_or(0, Vec::len);
        let completed = tasks.map_or(0, |tasks| {
            tasks
                .iter()
                .filter(|task| {
                    self.fetched
                        .get(&format!("{ACTIONS}{}", guild.id))
                        .is_some_and(|body| {
                            !body["channel_actions"][&task.channel_id]["completed"].is_null()
                        })
                })
                .count()
        });
        let mut rows = div().flex_none().flex().flex_col();
        if new_member(guild, chrono::Utc::now().timestamp_millis()) && total > 0 {
            rows = rows
                .child(
                    div()
                        .id("guide-progress")
                        .parity("guide-progress")
                        .mx(px(8.))
                        .w(px(286.))
                        .mt(px(-8.))
                        .p(px(8.))
                        .h(px(46.))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| this.open_guild_guide(cx)))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .h(px(18.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child("Get Started")
                                .child(div().flex_1())
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(500.))
                                        .child(format!("{completed} of {total}")),
                                )
                                .child(icon("menu_caret_right", 16., "--text-strong")),
                        )
                        .child(
                            div()
                                .h(px(4.))
                                .rounded(px(3.))
                                .overflow_hidden()
                                .bg(color("--background-mod-normal"))
                                .child(
                                    div()
                                        .h_full()
                                        .w(gpui::relative(
                                            (completed as f32 / total as f32 + 0.03).min(1.),
                                        ))
                                        .bg(gpui::linear_gradient(
                                            90.,
                                            gpui::linear_color_stop(super::hsla("--green-300"), 0.),
                                            gpui::linear_color_stop(super::hsla("--green-230"), 1.),
                                        )),
                                ),
                        ),
                )
                .child(
                    div()
                        .h(px(1.))
                        .my(px(4.))
                        .mr(px(9.))
                        .bg(color("--background-mod-subtle")),
                );
        }
        Some(
            rows.child(
                div().flex_none().h(px(34.)).pt(px(1.)).pl(px(8.)).child(
                    div()
                        .id("guild-guide-link")
                        .parity("guild-guide-link")
                        .h(px(32.))
                        .w(px(286.))
                        .px(px(8.))
                        .rounded(px(8.))
                        .when(self.guild_guide_open, |row| {
                            row.bg(color("--background-mod-normal"))
                        })
                        .hover(|style| style.bg(color("--background-mod-subtle")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .on_click(cx.listener(|this, _, _, cx| this.open_guild_guide(cx)))
                        .child(icon(
                            "ob_signpost",
                            20.,
                            if self.guild_guide_open {
                                "--text-strong"
                            } else {
                                "--channels-default"
                            },
                        ))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(500.))
                                .text_color(color(if self.guild_guide_open {
                                    "--text-strong"
                                } else {
                                    "--channels-default"
                                }))
                                .child("Server Guide"),
                        ),
                ),
            )
            .into_any_element(),
        )
    }

    pub(super) fn guild_guide_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(guild) = self.current_guild() else {
            return div().into_any_element();
        };
        let id = guild.id;
        let width = (self.viewport.0 - 72. - 303. - 16. - 64.).clamp(0., 1128.);
        let column = width.min(704.);
        let art_width = width + 32.;
        let art_height = art_width
            * if guild.home_header.is_some() {
                0.25
            } else {
                0.12
            };
        let art_url = guild
            .home_header
            .as_ref()
            .map(|hash| {
                format!("https://cdn.discordapp.com/home-headers/{id}/{hash}.png?size=1280")
            })
            .unwrap_or_else(|| {
                format!(
                    "https://discord.com/assets/{}.svg",
                    if super::theme() == dc_tokens::Theme::Light {
                        "3655f5d8659277b4"
                    } else {
                        "36eaea47ad7cb391"
                    }
                )
            });
        let banner = div()
            .parity("guide-banner")
            .ml(px(-16.))
            .w(px(art_width))
            .h(px(art_height))
            .rounded(px(16.))
            .overflow_hidden()
            .bg(color("--background-base-lowest"))
            .children(self.images.get(&art_url).cloned().map(|picture| {
                let source = BannerSource {
                    image: picture,
                    placeholder: guild.home_header.is_none(),
                };
                img(move |window: &mut gpui::Window, cx: &mut gpui::App| {
                    window
                        .use_asset::<BannerImage>(&source, cx)
                        .flatten()
                        .map(Ok)
                })
                .w(px(art_width))
                .h(px(art_height))
                .rounded(px(16.))
            }));
        let guild_icon = div()
            .parity_with_border("guide-icon", 4.)
            .relative()
            .mt(px(-50.))
            .mb(px(16.))
            .size(px(108.))
            .border(px(4.))
            .border_color(color("--background-base-low"))
            .rounded(px(30.))
            .bg(color("--background-base-low"))
            .children(
                self.images
                    .get(&format!("guide-icon:{id}"))
                    .cloned()
                    .map(|picture| img(picture).size(px(100.)).rounded(px(26.))),
            );
        let name = div()
            .parity("guide-name")
            .w(px(column))
            .h(px(40.))
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(32.))
            .line_height(px(40.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-strong"))
            .child(guild.name.clone())
            .children(super::tooltip::badge_image(guild).map(|badge| img(badge).size(px(24.))));
        let header = div()
            .w(px(width))
            .mt(px(-16.))
            .child(banner)
            .child(div().w(px(column)).mx_auto().child(guild_icon).child(name));
        let mut content = div().w(px(column)).flex().flex_col();
        if let Some(home) = self.guide_home()
            && new_member(guild, chrono::Utc::now().timestamp_millis())
        {
            if let Some(welcome) = &home.welcome
                && let Some(author) = welcome
                    .author_ids
                    .first()
                    .and_then(|id| id.parse::<Id>().ok())
                    .and_then(|id| self.stores.users.get(&id))
            {
                let current = self
                    .stores
                    .current_user
                    .as_ref()
                    .map(|user| user.display_name())
                    .unwrap_or("");
                let (message, mentions) = welcome_text(&welcome.message, current);
                let mut pieces = Vec::new();
                let mut at = 0;
                let piece = |text: &str, weight| {
                    self.piece(
                        &dc_markdown::Span {
                            text: text.to_owned(),
                            style: Default::default(),
                            kind: dc_markdown::Kind::Text,
                        },
                        super::hsla("--text-default"),
                        16.,
                        weight,
                        16.,
                    )
                };
                for range in mentions {
                    if range.start > at {
                        pieces.push(piece(&message[at..range.start], 500.));
                    }
                    pieces.push(piece(&message[range.clone()], 600.));
                    at = range.end;
                }
                if at < message.len() {
                    pieces.push(piece(&message[at..], 500.));
                }
                content = content.child(
                    div()
                        .parity("guide-welcome")
                        .relative()
                        .rounded(px(8.))
                        .bg(color("--card-background-default"))
                        .child(
                            div()
                                .pt(px(4.))
                                .px(px(16.))
                                .pb(px(16.))
                                .child(
                                    div()
                                        .pl(px(58.))
                                        .mb(px(10.))
                                        .h(px(18.))
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(self.guild_display_name(author)),
                                )
                                .child(
                                    div()
                                        .parity("guide-welcome-text")
                                        .w(px(column - 32.))
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-default"))
                                        .child(crate::inline::inline(pieces, px(20.))),
                                ),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(20.))
                                .top(px(-20.))
                                .size(px(48.))
                                .rounded_full()
                                .bg(color("--background-base-low"))
                                .children(
                                    self.images
                                        .get(&avatar_source(author).0)
                                        .cloned()
                                        .map(|picture| img(picture).size_full().rounded_full()),
                                ),
                        ),
                );
            }
            let mut tasks = div().py(px(16.)).flex().flex_col().gap(px(8.)).child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Get Started"),
            );
            for task in home.tasks.into_iter().flatten() {
                let Some(channel) = task
                    .channel_id
                    .parse::<Id>()
                    .ok()
                    .and_then(|id| guild.channels.iter().find(|channel| channel.id == id))
                else {
                    continue;
                };
                let channel_id = channel.id;
                let complete = self
                    .fetched
                    .get(&format!("{ACTIONS}{id}"))
                    .is_some_and(|body| {
                        body["channel_actions"][&task.channel_id]["completed"] == true
                    });
                tasks = tasks.child(
                    div()
                        .id(SharedString::from(format!("guide-task-{channel_id}")))
                        .h(px(64.))
                        .px(px(20.))
                        .py(px(12.))
                        .rounded(px(8.))
                        .bg(color("--card-background-default"))
                        .hover(|style| style.bg(color("--background-mod-subtle")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select(id, Some(channel_id));
                            cx.notify();
                        }))
                        .child(
                            div()
                                .size(px(40.))
                                .flex_none()
                                .rounded(px(20.))
                                .bg(color("--background-base-lowest"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(
                                    super::server_onboarding::channel_glyph(
                                        guild,
                                        channel,
                                        guild.rules_channel_id,
                                    ),
                                    20.,
                                    "--channel-icon",
                                )),
                        )
                        .child(
                            div()
                                .ml(px(8.))
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(task.title),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(format!("#{}", channel.name)),
                                ),
                        )
                        .child(if complete {
                            icon("ob_circle_check", 24., "--status-positive").into_any_element()
                        } else {
                            div()
                                .size(px(20.))
                                .m(px(2.))
                                .rounded_full()
                                .border(px(2.))
                                .border_color(color("--icon-muted"))
                                .into_any_element()
                        }),
                );
            }
            if let Some(rules) = guild.rules_channel_id {
                tasks = tasks.child(
                    div()
                        .id("guide-read-rules")
                        .h(px(64.))
                        .px(px(20.))
                        .py(px(12.))
                        .rounded(px(8.))
                        .bg(color("--card-background-default"))
                        .hover(|style| style.bg(color("--background-mod-subtle")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select(id, Some(rules));
                            cx.notify();
                        }))
                        .child(
                            div()
                                .size(px(40.))
                                .rounded(px(20.))
                                .bg(color("--background-base-lowest"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("ob_book_check", 16., "--channel-icon")),
                        )
                        .child(
                            div()
                                .ml(px(8.))
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child("Read the rules"),
                        ),
                );
            }
            content = content.child(tasks);
        }
        div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .bg(color(if super::theme() == dc_tokens::Theme::Midnight {
                "--home-background"
            } else {
                "--background-base-lower"
            }))
            .child(
                div()
                    .parity_below("guide-header", 1.)
                    .h(px(49.))
                    .flex_none()
                    .px(px(24.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("ob_signpost", 16., "--icon-muted"))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Server Guide"),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .child(
                        div()
                            .id("guild-guide-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.guild_guide_scroll)
                            .pr(px(16.))
                            .child(
                                div()
                                    .p(px(32.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(32.))
                                    .child(header)
                                    .child(content),
                            ),
                    )
                    .child(super::settings::auto_scrollbar(
                        self.guild_guide_scroll.clone(),
                    )),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn banner_center_crop_matches_placeholder_and_custom_aspect_ratios() {
        assert_eq!(banner_crop(1100, 275, true), (0, 71, 1100, 132));
        assert_eq!(banner_crop(1280, 320, false), (0, 0, 1280, 320));
        assert_eq!(banner_crop(800, 100, false), (200, 0, 400, 100));
    }

    #[test]
    fn welcome_collapses_html_whitespace_and_highlights_only_placeholders() {
        let (text, highlights) = welcome_text(
            " Hey [@username],\n\tWelcome  now\u{a0}friend @fixture ",
            "fixture",
        );
        assert_eq!(text, "Hey @fixture, Welcome now\u{a0}friend @fixture");
        assert_eq!(highlights, vec![4..12]);
    }

    fn guild() -> Guild {
        let mut stores = dc_state::Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user":{"id":"1","username":"fixture"},
                "guilds":[{"id":"100", "name":"fixture", "channels":[],
                    "features":["COMMUNITY","GUILD_SERVER_GUIDE","GUILD_ONBOARDING"]}],
                "merged_members":[[{"user_id":"1", "joined_at":"2026-11-01T00:00:00Z", "flags":10}]]
            }),
        );
        stores.guilds.remove(0)
    }

    #[test]
    fn guide_requires_features_and_new_member_window() {
        let mut guild = guild();
        let joined = chrono::DateTime::parse_from_rfc3339(&guild.joined_at)
            .unwrap()
            .timestamp_millis();
        assert!(enabled(&guild));
        assert!(new_member(&guild, joined + 7 * 24 * 60 * 60 * 1000 - 1));
        assert!(!new_member(&guild, joined + 7 * 24 * 60 * 60 * 1000));
        guild.my_flags |= 64;
        assert!(!new_member(&guild, joined));
        guild
            .features
            .retain(|feature| feature != "GUILD_SERVER_GUIDE");
        assert!(!enabled(&guild));
    }
}
