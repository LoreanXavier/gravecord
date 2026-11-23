//! Server Settings > Server Web Page (module 989433 `uT`/`uv`; checklist `uh`/`ug`, reason row `uA`/`uI`, platforms `ub`,
//! bar `u_`). Measured in `design/dom/settings/guild-landing*-darker-cozy.json`.
//!
//! The page lists only for a server that can be discovered or whose page is already published (`DISCOVERABLE` or
//! `is_published`). Its form edits the listing the Access page also holds (`discovery`): the category, the cover image
//! and the description go through `PATCH /guilds/{id}`, everything else through `PATCH /guilds/{id}/discovery-metadata`
//! (the subcategories alone through their own `PUT`/`DELETE` the moment they are picked).
//!
//! Not built: the published page's own address row (it needs the slug the web endpoint sends), the vanity-URL modal
//! that comes before publishing, and the minimum-length hints on the reasons.

use super::server_discovery::{DEFAULT_SPLASH, Meta, Select, splash_url};
use super::server_settings::Page;
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use dc_state::{Id, permissions};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, deferred,
    div, img, prelude::*, px,
};
use serde_json::{Value, json};

/// `ub`: the platforms a link can name, with the address each starts from.
const PLATFORMS: [(&str, &str, &str); 13] = [
    ("facebook", "Facebook", "https://facebook.com/"),
    ("instagram", "Instagram", "https://instagram.com/"),
    ("twitter", "Twitter", "https://twitter.com/"),
    ("tiktok", "TikTok", "https://tiktok.com/"),
    ("twitch", "Twitch", "https://twitch.tv/"),
    ("patreon", "Patreon", "https://patreon.com/"),
    ("reddit", "Reddit", "https://reddit.com/r/"),
    ("youtube", "YouTube", "https://youtube.com/"),
    ("steamcommunity", "Steam", "https://steamcommunity.com/"),
    ("soundcloud", "SoundCloud", "https://soundcloud.com/"),
    ("bandcamp", "Bandcamp", "https://bandcamp.com/"),
    ("github", "GitHub", "https://github.com/"),
    ("etsy", "Etsy", "https://etsy.com/"),
];

const MAX_LINKS: usize = 9;
const ABOUT_MAX: usize = 2400;
const ABOUT_MIN: usize = 300;
const REASON_MAX: usize = 128;
const REASON_MIN: usize = 10;
const DESCRIPTION_PLACEHOLDER: &str =
    "This server is all about hanging with friends, sharing pet pics, and Gregorian chanting.";
const REASON_PLACEHOLDERS: [&str; 4] = [
    "ex: We hold weekly study group sessions",
    "ex: Tips for keeping your house plants healthy",
    "ex: Feedback on your art",
    "ex: Find other players for your group",
];

/// `FORCE_LANDING` in a parity run: 0 is the server as it is, 1 an unpublished page, 2 a published one.
pub(crate) static FORCE_LANDING: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

#[derive(Default)]
pub(crate) struct LandingState {
    about: Option<Entity<Editor>>,
    reasons: Vec<Entity<Editor>>,
    social: Vec<Entity<Editor>>,
    /// The platform each link names, by `PLATFORMS` position (`x`).
    platforms: Vec<Option<usize>>,
    /// The link whose platform list is open.
    platform_open: Option<usize>,
    /// The checklist is folded (`r` in `uh`).
    folded: bool,
    _events: Vec<Subscription>,
}

impl LandingState {
    /// Every text box of the page, for the harness to find the focused one.
    #[cfg(feature = "capture")]
    pub(super) fn editors(&self) -> Vec<Entity<Editor>> {
        let mut list: Vec<Entity<Editor>> = self.about.iter().cloned().collect();
        list.extend(self.reasons.iter().cloned());
        list.extend(self.social.iter().cloned());
        list
    }
}

/// The platform a link's address names (the closure in `uv`'s effect).
fn platform_of(link: &str) -> Option<usize> {
    let rest = link.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?.split(':').next()?;
    let mut parts: Vec<&str> = host.split('.').collect();
    if parts.len() == 3 && parts[1].eq_ignore_ascii_case("bandcamp") {
        return PLATFORMS.iter().position(|(key, _, _)| *key == "bandcamp");
    }
    let mut first = parts.remove(0).to_lowercase();
    if first == "www" && !parts.is_empty() {
        first = parts.remove(0).to_lowercase();
    }
    PLATFORMS.iter().position(|(key, _, _)| *key == first)
}

/// What a link's box shows after its platform's address.
fn link_tail(link: &str, platform: Option<usize>) -> String {
    match platform {
        Some(at) => link
            .strip_prefix(PLATFORMS[at].2)
            .unwrap_or(link)
            .to_owned(),
        None => link.to_owned(),
    }
}

/// `onChange` of a link's box: the address that follows.
fn typed_link(old: &str, typed: &str) -> Option<String> {
    let mut link = typed.to_owned();
    if old.is_empty() && link.len() > 1 && !link.starts_with("http:") && !link.starts_with("https:")
    {
        link = format!("https://{link}");
    }
    let lower = link.to_lowercase();
    let blocked = [
        "zip", "exe", "pdf", "dmg", "msi", "apk", "rar", "7z", "tar", "gz", "iso", "doc", "docx",
        "xls", "xlsx", "ppt", "pptx", "mp4", "mp3",
    ];
    if blocked
        .iter()
        .any(|extension| lower.ends_with(&format!(".{extension}")))
    {
        return None;
    }
    Some(link)
}

/// `b` in `uv`: a reason is ready with 10 to 128 characters.
fn reason_ready(reason: &Value) -> bool {
    let text = reason["reason"].as_str().unwrap_or_default();
    let length = text.chars().count();
    (REASON_MIN..=REASON_MAX).contains(&length)
}

fn reason_empty(reason: &Value) -> bool {
    reason["reason"].as_str().is_none_or(str::is_empty) && reason["emoji_name"].is_null()
}

/// `C` in `uv`: the four requirements and whether each is met.
fn requirements(splash: bool, description: bool, meta: &Meta) -> [(&'static str, bool); 4] {
    let about = meta.about.as_str().map_or(0, |about| about.chars().count());
    let reasons: Vec<Value> = meta.reasons.as_array().cloned().unwrap_or_default();
    let reasons_met = reasons
        .iter()
        .all(|reason| reason_empty(reason) || reason_ready(reason))
        && reasons.iter().filter(|reason| reason_ready(reason)).count() >= 2;
    [
        ("Upload a cover image", splash),
        ("Short description", description),
        (
            "About your server",
            (ABOUT_MIN..=ABOUT_MAX).contains(&about),
        ),
        ("Reasons to join", reasons_met),
    ]
}

fn links(meta: &Meta) -> Vec<String> {
    meta.social
        .as_array()
        .map(|list| {
            list.iter()
                .map(|link| link.as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

impl Root {
    /// Escape: an open social-link platform list closes before the page does.
    pub(super) fn close_landing_layer(&mut self) -> bool {
        self.server_settings
            .as_mut()
            .is_some_and(|open| open.landing.platform_open.take().is_some())
    }

    fn landing_meta_mut(&mut self) -> Option<&mut Meta> {
        self.server_settings
            .as_mut()
            .map(|open| &mut open.discovery.meta)
    }

    fn landing_editor(
        placeholder: &str,
        text_area: bool,
        cx: &mut Context<Self>,
    ) -> Entity<Editor> {
        cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder(placeholder.to_owned());
            editor.set_reports_all_edits(true);
            if text_area {
                editor.set_enter_inserts_newline(true);
            }
            editor
        })
    }

    /// The text boxes for the listing as it was read: the About text, the four reasons and a box per link.
    pub(super) fn landing_sync(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.server_settings.as_ref() else {
            return;
        };
        let meta = open.discovery.meta.clone();
        let mut events: Vec<Subscription> = Vec::new();
        let about = Self::landing_editor(DESCRIPTION_PLACEHOLDER, true, cx);
        about.update(cx, |editor, _| {
            editor.set_text(meta.about.as_str().unwrap_or_default())
        });
        events.push(
            cx.subscribe(&about, |this, editor, event: &EditorEvent, cx| {
                if let EditorEvent::Edited = event {
                    let text = editor.read(cx).text().to_owned();
                    let cut: String = text.chars().take(ABOUT_MAX).collect();
                    if cut != text {
                        editor.update(cx, |editor, _| editor.set_text(&cut));
                    }
                    if let Some(meta) = this.landing_meta_mut() {
                        meta.about = json!(cut);
                    }
                    cx.notify();
                }
            }),
        );
        let reasons_now: Vec<Value> = meta.reasons.as_array().cloned().unwrap_or_default();
        let mut reasons = Vec::new();
        for (index, placeholder) in REASON_PLACEHOLDERS.iter().enumerate() {
            let editor = Self::landing_editor(placeholder, false, cx);
            let text = reasons_now
                .get(index)
                .and_then(|reason| reason["reason"].as_str())
                .unwrap_or_default()
                .to_owned();
            editor.update(cx, |editor, _| editor.set_text(&text));
            events.push(
                cx.subscribe(&editor, move |this, editor, event: &EditorEvent, cx| {
                    if let EditorEvent::Edited = event {
                        let text = editor.read(cx).text().to_owned();
                        let cut: String = text.chars().take(REASON_MAX).collect();
                        if cut != text {
                            editor.update(cx, |editor, _| editor.set_text(&cut));
                        }
                        this.landing_set_reason(index, |reason| reason["reason"] = json!(cut));
                        cx.notify();
                    }
                }),
            );
            reasons.push(editor);
        }
        let current = links(&meta);
        let mut social = Vec::new();
        let mut platforms = Vec::new();
        for (index, link) in current.iter().enumerate() {
            let platform = platform_of(link);
            let editor = Self::landing_editor("ex: https://twitter.com/discord", false, cx);
            let tail = link_tail(link, platform);
            editor.update(cx, |editor, _| {
                editor.set_text(&tail);
                if platform.is_some() {
                    editor.set_placeholder(String::new());
                }
            });
            events.push(
                cx.subscribe(&editor, move |this, editor, event: &EditorEvent, cx| {
                    if let EditorEvent::Edited = event {
                        let text = editor.read(cx).text().to_owned();
                        let cut: String = text.chars().take(150).collect();
                        if cut != text {
                            editor.update(cx, |editor, _| editor.set_text(&cut));
                        }
                        this.landing_typed(index, &cut, cx);
                    }
                }),
            );
            social.push(editor);
            platforms.push(platform);
        }
        if let Some(open) = self.server_settings.as_mut() {
            open.landing.about = Some(about);
            open.landing.reasons = reasons;
            open.landing.social = social;
            open.landing.platforms = platforms;
            open.landing._events = events;
        }
        for reason in &reasons_now {
            if let Some(name) = reason["emoji_name"].as_str() {
                self.request_emoji_image(name);
            }
        }
    }

    /// `ux.MA`: the reasons list with one entry changed; an empty list starts as four empty entries.
    fn landing_set_reason(&mut self, index: usize, change: impl FnOnce(&mut Value)) {
        let Some(meta) = self.landing_meta_mut() else {
            return;
        };
        let mut list: Vec<Value> = meta.reasons.as_array().cloned().unwrap_or_default();
        while list.len() < 4 {
            list.push(json!({"reason": "", "emoji_name": null}));
        }
        if let Some(entry) = list.get_mut(index) {
            change(entry);
        }
        meta.reasons = Value::Array(list);
    }

    pub(super) fn landing_set_reason_emoji(
        &mut self,
        index: usize,
        emoji: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(name) = emoji.as_deref() {
            self.request_emoji_image(name);
        }
        self.landing_set_reason(index, |reason| reason["emoji_name"] = json!(emoji));
        cx.notify();
    }

    fn landing_clear_reason(&mut self, index: usize, cx: &mut Context<Self>) {
        self.landing_set_reason(index, |reason| {
            *reason = json!({"reason": "", "emoji_name": null})
        });
        if let Some(editor) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.landing.reasons.get(index).cloned())
        {
            editor.update(cx, |editor, _| editor.set_text(""));
        }
        cx.notify();
    }

    fn landing_typed(&mut self, index: usize, text: &str, cx: &mut Context<Self>) {
        let platform = self
            .server_settings
            .as_ref()
            .and_then(|open| open.landing.platforms.get(index).copied())
            .flatten();
        let Some(meta) = self.landing_meta_mut() else {
            return;
        };
        let mut current = links(meta);
        let Some(old) = current.get(index).cloned() else {
            return;
        };
        let wanted = match platform {
            Some(at) => format!("{}{text}", PLATFORMS[at].2),
            None => text.to_owned(),
        };
        match typed_link(&old, &wanted) {
            Some(link) => {
                current[index] = link;
                meta.social = json!(current);
            }
            None => {
                // The address is refused: the box keeps what it had.
                let tail = link_tail(&old, platform);
                if let Some(editor) = self
                    .server_settings
                    .as_ref()
                    .and_then(|open| open.landing.social.get(index).cloned())
                {
                    editor.update(cx, |editor, _| editor.set_text(&tail));
                }
            }
        }
        cx.notify();
    }

    fn landing_pick_platform(&mut self, index: usize, platform: usize, cx: &mut Context<Self>) {
        let Some(meta) = self.landing_meta_mut() else {
            return;
        };
        let mut current = links(meta);
        if let Some(link) = current.get_mut(index) {
            *link = PLATFORMS[platform].2.to_owned();
        }
        meta.social = json!(current);
        if let Some(open) = self.server_settings.as_mut() {
            if let Some(slot) = open.landing.platforms.get_mut(index) {
                *slot = Some(platform);
            }
            open.landing.platform_open = None;
            if let Some(editor) = open.landing.social.get(index).cloned() {
                editor.update(cx, |editor, _| {
                    editor.set_text("");
                    editor.set_placeholder(String::new());
                });
            }
        }
        cx.notify();
    }

    fn landing_add_link(&mut self, cx: &mut Context<Self>) {
        let Some(meta) = self.landing_meta_mut() else {
            return;
        };
        let mut current = links(meta);
        if current.len() >= MAX_LINKS {
            return;
        }
        current.push(String::new());
        meta.social = json!(current);
        self.landing_sync(cx);
        cx.notify();
    }

    fn landing_remove_link(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(meta) = self.landing_meta_mut() else {
            return;
        };
        let mut current = links(meta);
        if index < current.len() {
            current.remove(index);
        }
        meta.social = json!(current);
        if let Some(open) = self.server_settings.as_mut() {
            open.landing.platform_open = None;
        }
        self.landing_sync(cx);
        cx.notify();
    }

    /// `DISCOVERABLE` or a published page, for someone who can manage the server (`DISCOVERY_LANDING_PAGE`'s predicate).
    pub(super) fn landing_listed(&self, guild: Id) -> bool {
        let Some(open) = self.server_settings.as_ref() else {
            return false;
        };
        if FORCE_LANDING.load(std::sync::atomic::Ordering::Relaxed) != 0 {
            return true;
        }
        let Some((known, me)) = self.stores.guild(guild).zip(self.stores.me()) else {
            return false;
        };
        permissions::can_in_guild(known, me, permissions::MANAGE_GUILD)
            && (known
                .features
                .iter()
                .any(|feature| feature == "DISCOVERABLE")
                || open.discovery.meta.published
                || open.page == Page::Landing)
    }

    fn landing_published(&self) -> bool {
        match FORCE_LANDING.load(std::sync::atomic::Ordering::Relaxed) {
            1 => return false,
            2 => return true,
            _ => {}
        }
        self.server_settings
            .as_ref()
            .is_some_and(|open| open.discovery.meta.published)
    }

    pub(super) fn landing_dirty(&self) -> bool {
        self.discovery_dirty()
    }

    pub(super) fn landing_bar_message(&self) -> Option<String> {
        self.server_settings
            .as_ref()
            .and_then(|open| open.discovery.error.clone())
    }

    pub(super) fn request_landing(&mut self, guild: Id) {
        self.request_discovery(guild);
        self.request_discovery_art();
    }

    pub(super) fn reset_landing(&mut self, guild: Id) {
        self.request_landing(guild);
    }

    /// `u_`'s save: the cover and the description, then the listing without its empty links.
    pub(super) fn save_landing(&mut self, guild: Id) {
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        let discovery = &mut open.discovery;
        if discovery.saved_bits.is_none() || discovery.saving {
            return;
        }
        discovery.error = None;
        discovery.saving = true;
        let body = json!({
            "discovery_splash": discovery.bits.splash,
            "description": discovery.bits.description,
        });
        self.landing_send_guild(guild, body);
    }

    /// `I`: publishing or taking the page down writes the listing with `is_published` changed, then the guild.
    fn landing_publish(&mut self, guild: Id, publish: bool) {
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        let discovery = &mut open.discovery;
        discovery.meta.published = publish;
        discovery.error = None;
        discovery.saving = true;
        if publish {
            let body = json!({
                "discovery_splash": discovery.bits.splash,
                "description": discovery.bits.description,
            });
            self.landing_send_guild(guild, body);
        } else {
            // Taking the page down touches the listing alone: a guild write would publish it again.
            let body = discovery.meta.patch_body();
            self.discovery_send(
                format!("{}{guild}", super::server_discovery::SAVED_META_PREFIX),
                reqwest::Method::PATCH,
                format!("/guilds/{guild}/discovery-metadata"),
                Some(body),
            );
        }
    }

    fn landing_send_guild(&mut self, guild: Id, body: Value) {
        let _ = self.requests.send(crate::net::Request::Rest {
            key: Some(format!(
                "{}{guild}",
                super::server_discovery::SAVED_GUILD_PREFIX
            )),
            method: reqwest::Method::PATCH,
            route: format!("/guilds/{guild}"),
            body: Some(body),
        });
    }

    fn landing_heading(&self) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("Server Web Page"),
                    )
                    .child(div().ml(px(8.)).child(beta_badge())),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-subtle"))
                    .child(self.landing_intro()),
            )
            .into_any_element()
    }

    fn landing_intro(&self) -> AnyElement {
        let before = "Help people find and join your community more easily with a landing page on ";
        let middle = ". Your page won\u{2019}t go live until it\u{2019}s published and can be disabled at any time. ";
        let (first, second) = ("discord.com", "Learn more");
        let text = format!("{before}{first}{middle}{second}.");
        let one = before.len()..before.len() + first.len();
        let start = one.end + middle.len();
        let two = start..start + second.len();
        let link = gpui::HighlightStyle {
            color: Some(color("--text-link").into()),
            ..Default::default()
        };
        let styled =
            gpui::StyledText::new(text).with_highlights([(one.clone(), link), (two.clone(), link)]);
        gpui::InteractiveText::new("landing-intro", styled)
            .on_click(vec![one, two], |which, _, cx| {
                cx.open_url(if which == 0 {
                    "https://discord.com"
                } else {
                    "https://support.discord.com/hc/en-us/articles/4673515000983"
                })
            })
            .into_any_element()
    }

    /// `i8.D`: a label with its description, and the control 8px under them.
    fn landing_section(
        label: &'static str,
        description: Option<&'static str>,
        control: AnyElement,
    ) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(Self::landing_label(label, description))
            .child(control)
    }

    fn landing_label(label: &'static str, description: Option<&'static str>) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(label),
            )
            .when_some(description, |this, text| {
                this.child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child(text),
                )
            })
    }

    fn landing_divider() -> gpui::Div {
        div()
            .h(px(1.))
            .w_full()
            .flex_none()
            .bg(gpui::rgba(0x97979f1f))
    }

    fn landing_field(
        &self,
        editor: Option<Entity<Editor>>,
        height: f32,
        right: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let focus = editor.clone();
        let ring = editor
            .as_ref()
            .map(|editor| gpui::Focusable::focus_handle(editor.read(cx), cx));
        div()
            .id(SharedString::from(format!("landing-field-{height}")))
            .when_some(ring, |this, ring| {
                this.track_focus(&ring)
                    .focus(|style| style.border_color(color("--text-brand")))
            })
            .h(px(height))
            .w_full()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--input-border-default"))
            .bg(color("--input-background-default"))
            .cursor_text()
            .on_click(cx.listener(move |_, _, window, cx| {
                if let Some(focus) = focus.as_ref() {
                    let handle = gpui::Focusable::focus_handle(focus.read(cx), cx);
                    window.focus(&handle, cx);
                }
            }))
            .child(
                div()
                    .size_full()
                    .pl(px(10.))
                    .pr(px(right))
                    .py(px(12.))
                    .children(editor.map(|editor| div().w_full().child(editor))),
            )
            .into_any_element()
    }

    fn landing_reason_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(open) = self.server_settings.as_ref() else {
            return div().into_any_element();
        };
        let editor = open.landing.reasons.get(index).cloned();
        let reason = open
            .discovery
            .meta
            .reasons
            .as_array()
            .and_then(|list| list.get(index))
            .cloned()
            .unwrap_or_else(|| json!({"reason": "", "emoji_name": null}));
        let name = reason["emoji_name"].as_str().map(str::to_owned);
        let picture = name
            .as_deref()
            .and_then(|name| self.images.get(&format!("emoji:{name}")).cloned());
        let filled = !reason_empty(&reason);
        let probe = SharedString::from(format!("landing-reason-emoji-{index}"));
        let focus = editor.clone();
        let ring = editor
            .as_ref()
            .map(|editor| gpui::Focusable::focus_handle(editor.read(cx), cx));
        div()
            .id(SharedString::from(format!("landing-reason-{index}")))
            .when_some(ring, |this, ring| {
                this.track_focus(&ring)
                    .focus(|style| style.border_color(color("--text-brand")))
            })
            .h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--input-border-default"))
            .bg(color("--input-background-default"))
            .pl(px(4.))
            .pr(px(8.))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .id(probe.clone())
                    .relative()
                    .child(self.anchor_probe(probe.clone()))
                    .size(px(30.))
                    .flex_none()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when_some(
                        picture.or_else(|| {
                            name.is_none()
                                .then(|| self.images.get("emoji_button").cloned())
                                .flatten()
                        }),
                        |this, picture| this.child(img(picture).size(px(20.))),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let key = format!("landing-reason-emoji-{index}");
                        let anchor = this.tip_anchors.borrow().get(key.as_str()).copied();
                        if let Some(bounds) = anchor {
                            this.open_picker(
                                super::picker::Target::ReasonEmoji(index as u8),
                                gpui::point(bounds.left() + px(5.), bounds.top()),
                                window,
                                cx,
                            );
                        }
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .cursor_text()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |_, _, window, cx| {
                            if let Some(focus) = focus.as_ref() {
                                let handle = gpui::Focusable::focus_handle(focus.read(cx), cx);
                                window.focus(&handle, cx);
                            }
                        }),
                    )
                    .children(editor),
            )
            .when(filled, |this| {
                this.child(
                    div()
                        .id(SharedString::from(format!("landing-reason-clear-{index}")))
                        .cursor_pointer()
                        .child(icon("search_clear", 16., "--icon-muted"))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.landing_clear_reason(index, cx)),
                        ),
                )
            })
            .into_any_element()
    }

    fn landing_link_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(open) = self.server_settings.as_ref() else {
            return div().into_any_element();
        };
        let platform = open.landing.platforms.get(index).copied().flatten();
        let editor = open.landing.social.get(index).cloned();
        let opened = open.landing.platform_open == Some(index);
        let probe = SharedString::from(format!("landing-platform-{index}"));
        let focus = editor.clone();
        let ring = editor
            .as_ref()
            .map(|editor| gpui::Focusable::focus_handle(editor.read(cx), cx));
        div()
            .flex()
            .gap(px(8.))
            .child(
                div()
                    .id(probe.clone())
                    .relative()
                    .child(self.anchor_probe(probe))
                    .w(px(175.))
                    .flex_none()
                    .h(px(40.))
                    .pl(px(12.))
                    .pr(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if opened {
                        color("--text-brand")
                    } else {
                        color("--input-border-default")
                    })
                    .bg(color("--input-background-default"))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .pr(px(10.))
                            .truncate()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(match platform {
                                Some(_) => color("--text-strong"),
                                None => color("--input-placeholder-text-default"),
                            })
                            .child(match platform {
                                Some(at) => PLATFORMS[at].1,
                                None => "Select a social platform",
                            }),
                    )
                    .child(
                        div()
                            .w(px(32.))
                            .h(px(38.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                gpui::svg()
                                    .path("icons/select_chevron.svg")
                                    .size(px(20.))
                                    .text_color(color("--text-strong"))
                                    .with_transformation(gpui::Transformation::rotate(
                                        gpui::percentage(if opened { 0.5 } else { 0. }),
                                    )),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(open) = this.server_settings.as_mut() {
                            open.landing.platform_open =
                                (open.landing.platform_open != Some(index)).then_some(index);
                            open.discovery
                                .scroll
                                .set_offset(gpui::point(px(0.), px(0.)));
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id(SharedString::from(format!("landing-link-{index}")))
                    .when_some(ring, |this, ring| {
                        this.track_focus(&ring)
                            .focus(|style| style.border_color(color("--text-brand")))
                    })
                    .flex_1()
                    .min_w_0()
                    .h(px(40.))
                    .pl(px(if platform.is_some() { 8. } else { 12. }))
                    .pr(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--input-border-default"))
                    .bg(color("--input-background-default"))
                    .cursor_text()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .when(platform.is_none(), |this| this.opacity(1.))
                    .on_click(cx.listener(move |_, _, window, cx| {
                        if let Some(focus) = focus.as_ref() {
                            let handle = gpui::Focusable::focus_handle(focus.read(cx), cx);
                            window.focus(&handle, cx);
                        }
                    }))
                    .when_some(platform, |this, at| {
                        this.child(
                            div()
                                .flex_none()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-muted"))
                                .child(PLATFORMS[at].2),
                        )
                    })
                    .child(div().flex_1().min_w_0().children(editor))
                    .child(
                        div()
                            .id(SharedString::from(format!("landing-link-remove-{index}")))
                            .size(px(24.))
                            .flex_none()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(
                                "channel_settings_trash",
                                16.,
                                "--control-icon-only-icon-default",
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.landing_remove_link(index, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    /// `uv`: the form.
    pub(super) fn server_landing_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(open) = self.server_settings.as_ref() else {
            return div().into_any_element();
        };
        let discovery = &open.discovery;
        if !discovery.loaded() {
            return div().into_any_element();
        }
        let published = self.landing_published();
        // The description's box takes this page's words.
        if let Some(editor) = discovery.description.clone() {
            editor.update(cx, |editor, _| {
                editor.set_placeholder(DESCRIPTION_PLACEHOLDER.to_owned())
            });
        }
        let primary_name = discovery
            .categories
            .iter()
            .find(|category| category.id == discovery.meta.primary)
            .map(|category| category.name.clone());
        let secondary: Vec<String> = discovery
            .meta
            .secondary
            .iter()
            .filter_map(|id| {
                discovery
                    .categories
                    .iter()
                    .find(|category| category.id == *id)
                    .map(|category| category.name.clone())
            })
            .collect();
        let category = Self::landing_section(
            "Category",
            Some(
                "Categories help Discord users discover communities that interest them. If you\u{2019}ve already set one that you\u{2019}re happy with, there\u{2019}s no need to change it.",
            ),
            self.discovery_select(
                Select::Category,
                primary_name.into_iter().collect(),
                discovery.meta.primary != 0,
                cx,
            ),
        );
        let subcategories = Self::landing_section(
            "Subcategories",
            None,
            self.discovery_select(Select::Subcategories, secondary, false, cx),
        );
        let image = discovery
            .bits
            .splash
            .as_deref()
            .and_then(|hash| {
                if hash.starts_with("data:") {
                    self.images.get(hash).cloned()
                } else {
                    self.images.get(&splash_url(guild, hash)).cloned()
                }
            })
            .or_else(|| self.images.get(DEFAULT_SPLASH).cloned());
        let cover = div()
            .flex()
            .gap(px(24.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Self::landing_label(
                        "Discovery listing cover image",
                        Some("This image will be displayed on both your Discovery Listing and Server Web Page. The recommended minimum size is 1920x1080 and recommended aspect ratio is 16:9."),
                    )),
            )
            .child(
                div()
                    .w(px(if published { 279. } else { 230. }))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .id("landing-cover")
                            .w_full()
                            .h(px(if published { 156.9 } else { 129.375 }))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(color("--background-base-lower"))
                            .cursor_pointer()
                            .children(image.map(|image| {
                                img(image).size_full().object_fit(gpui::ObjectFit::Cover)
                            }))
                            .on_click(cx.listener(|this, _, _, cx| this.discovery_pick_splash(cx))),
                    )
                    .child(
                        div()
                            .id("landing-upload")
                            .h(px(32.))
                            .w_full()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-muted"))
                            .bg(color("--control-secondary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .child("Upload Background")
                            .on_click(cx.listener(|this, _, _, cx| this.discovery_pick_splash(cx))),
                    ),
            );
        let description = Self::landing_section(
            "Server Description",
            Some("Let people know what your server is about in one to two lines."),
            self.landing_field(discovery.description.clone(), 89., 10., cx),
        );
        let reasons = Self::landing_section(
            "Reasons to join your server",
            Some(
                "What are the fun activities that happen in your community? These will display on your server\u{2019}s web page.",
            ),
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .children((0..4).map(|index| self.landing_reason_row(index, cx)))
                .into_any_element(),
        );
        let about_length = discovery
            .meta
            .about
            .as_str()
            .map_or(0, |about| about.chars().count());
        let about = Self::landing_section(
            "About your community",
            Some(
                "Describe your community in more depth. What does a prospective member need to know, and why should they join?",
            ),
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(self.landing_field(open.landing.about.clone(), 236., 89.53, cx))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-muted"))
                        .child(format!("{about_length} / {ABOUT_MAX}")),
                )
                .into_any_element(),
        );
        let count = links(&discovery.meta).len();
        let social = Self::landing_section(
            "Social Links",
            Some(
                "Display up to 9 of your official social media accounts on your page by linking them here.",
            ),
            div()
                .flex()
                .flex_col()
                .gap(px(20.))
                .children((0..count).map(|index| self.landing_link_row(index, cx)))
                .child(
                    div()
                        .id("landing-add-link")
                        .h(px(32.))
                        .px(px(11.))
                        .self_start()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-muted"))
                        .bg(color("--control-secondary-background-default"))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-secondary-text-default"))
                        .when(count < MAX_LINKS, |this| this.cursor_pointer())
                        .when(count >= MAX_LINKS, |this| this.opacity(0.5))
                        .child("Add another")
                        .on_click(cx.listener(|this, _, _, cx| this.landing_add_link(cx))),
                )
                .into_any_element(),
        );
        let disable = published.then(|| {
            Self::landing_section(
                "Disable landing page",
                Some("Removes your landing page from the web. Publishing your page again will re-enable it."),
                div()
                    .id("landing-disable")
                    .h(px(32.))
                    .px(px(11.))
                    .self_start()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--control-critical-primary-border-default"))
                    .bg(color("--control-critical-primary-background-default"))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .child("Disable landing page")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.landing_publish(guild, false);
                        cx.notify();
                    }))
                    .into_any_element(),
            )
        });
        let width = if published { 626.6 } else { 485. };
        div()
            .w(px(757.))
            .flex()
            .child(
                div()
                    .w(px(width))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .child(self.landing_heading())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(40.))
                            .child(Self::landing_divider())
                            .child(category)
                            .child(subcategories)
                            .child(Self::landing_divider())
                            .child(cover)
                            .child(Self::landing_divider())
                            .child(description)
                            .child(Self::landing_divider())
                            .child(reasons)
                            .child(Self::landing_divider())
                            .child(about)
                            .child(Self::landing_divider())
                            .child(social)
                            .children(disable.map(|disable| {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(40.))
                                    .child(Self::landing_divider())
                                    .child(disable)
                            })),
                    ),
            )
            .into_any_element()
    }

    /// `uh`: the checklist beside an unpublished page. It sticks 16px under the top of the scroller.
    pub(super) fn landing_checklist(
        &self,
        guild: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let discovery = &open.discovery;
        if open.page != Page::Landing || !discovery.loaded() || self.landing_published() {
            return None;
        }
        let rows = requirements(
            discovery.bits.splash.is_some(),
            discovery
                .bits
                .description
                .as_deref()
                .is_some_and(|text| !text.is_empty()),
            &discovery.meta,
        );
        let done = rows.iter().filter(|(_, met)| *met).count();
        let valid = links(&discovery.meta)
            .iter()
            .all(|link| link.is_empty() || platform_of(link).is_some());
        let ready = done == rows.len() && valid;
        let folded = open.landing.folded;
        let scrolled = -f32::from(open.scroll.offset().y);
        let top = (92. - scrolled).max(48.) - 32.;
        let list = (!folded).then(|| {
            div()
                .mt(px(16.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child("Requirements"),
                )
                .children(rows.iter().enumerate().map(|(at, (text, met))| {
                    div()
                        .mt(px(if at == 0 { 24. } else { 20. }))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-strong"))
                                .child(*text),
                        )
                        .child(if *met {
                            div()
                                .ml(px(8.))
                                .child(icon("circle_check", 20., "--status-positive-background"))
                                .into_any_element()
                        } else {
                            div()
                                .ml(px(8.))
                                .size(px(20.))
                                .rounded_full()
                                .border_2()
                                .border_color(gpui::rgba(0x97979f1f))
                                .into_any_element()
                        })
                }))
        });
        Some(
            div()
                .absolute()
                .left(px(565.5))
                .top(px(top))
                .w(px(231.5))
                .child(
                    div()
                        .p(px(16.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(gpui::rgba(0x97979f1f))
                        .bg(color("--background-base-low"))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id("landing-checklist-fold")
                                .h(px(24.))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child("Landing Page"),
                                )
                                .child(
                                    gpui::svg()
                                        .path("icons/select_chevron.svg")
                                        .size(px(24.))
                                        .text_color(color("--text-subtle"))
                                        .with_transformation(gpui::Transformation::rotate(
                                            gpui::percentage(if folded { 0. } else { 0.5 }),
                                        )),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(open) = this.server_settings.as_mut() {
                                        open.landing.folded = !open.landing.folded;
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .mt(px(16.))
                                .h(px(4.))
                                .rounded(px(2.))
                                .bg(gpui::rgba(0x97979f1f))
                                .child(
                                    div()
                                        .h_full()
                                        .rounded(px(2.))
                                        .w(gpui::relative(done as f32 / rows.len() as f32))
                                        .bg(gpui::rgb(0x5865f2)),
                                ),
                        )
                        .children(list)
                        .child(div().mt(px(16.)).h(px(1.)).bg(gpui::rgba(0x97979f1f)))
                        .child(
                            div()
                                .id("landing-publish")
                                .mt(px(15.))
                                .h(px(32.))
                                .px(px(11.))
                                .self_start()
                                .rounded(px(8.))
                                .border_1()
                                .border_color(gpui::rgba(0xffffff14))
                                .bg(gpui::rgb(0x5865f2))
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(gpui::white())
                                .when(ready, |this| this.cursor_pointer())
                                .when(!ready, |this| this.opacity(0.5))
                                .child("Publish Landing Page")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if ready {
                                        this.landing_publish(guild, true);
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The open platform list of a link: the platforms no other link already names, five rows at a time.
    pub(super) fn landing_platform_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if open.page != Page::Landing {
            return None;
        }
        let index = open.landing.platform_open?;
        let key = format!("landing-platform-{index}");
        let field = self
            .tip_anchors
            .borrow()
            .get(key.as_str())
            .copied()?
            .dilate(px(1.));
        let used: Vec<usize> = open
            .landing
            .platforms
            .iter()
            .enumerate()
            .filter(|(at, _)| *at != index)
            .filter_map(|(_, platform)| *platform)
            .collect();
        let rows: Vec<AnyElement> = PLATFORMS
            .iter()
            .enumerate()
            .filter(|(at, _)| !used.contains(at))
            .map(|(at, (_, label, _))| {
                Self::discovery_option(
                    SharedString::from(format!("landing-platform-option-{at}")),
                    (*label).to_owned(),
                    open.landing.platforms.get(index).copied().flatten() == Some(at),
                    false,
                    false,
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.landing_pick_platform(index, at, cx)),
                )
                .into_any_element()
            })
            .collect();
        let space_below = (self.viewport.1 - f32::from(field.bottom()) - 12.).max(0.);
        let space_above = (f32::from(field.top()) - 12.).max(0.);
        let top = if space_below < 210. && space_above > space_below {
            (field.top() - px(4.) - px(210.)).max(px(8.))
        } else {
            field.bottom() + px(4.)
        };
        Some(
            deferred(
                div()
                    .id("landing-platform-layer")
                    .occlude()
                    .absolute()
                    .left(field.left())
                    .top(top)
                    .w(field.size.width)
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .shadow(vec![super::shadow_high()])
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(cx.listener(
                        move |this, event: &gpui::MouseDownEvent, _, cx| {
                            if !field.contains(&event.position)
                                && let Some(open) = this.server_settings.as_mut()
                            {
                                open.landing.platform_open = None;
                                cx.notify();
                            }
                        },
                    ))
                    .child(
                        div()
                            .relative()
                            .child(
                                div()
                                    .id("landing-platform-scroller")
                                    .max_h(px(200.))
                                    .pr(px(16.))
                                    .flex()
                                    .flex_col()
                                    .overflow_y_scroll()
                                    .track_scroll(&open.discovery.scroll)
                                    .children(rows),
                            )
                            .child(super::settings::auto_scrollbar(
                                open.discovery.scroll.clone(),
                            )),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

/// The `BETA` tag after the page's name and in its sidebar entry: a brand pill (`badge primary`).
pub(super) fn beta_badge() -> AnyElement {
    div()
        .h(px(16.))
        .px(px(8.))
        .rounded_full()
        .bg(gpui::rgb(0x5865f2))
        .flex()
        .items_center()
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight(600.))
        .text_color(gpui::white())
        .child("BETA")
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_names_its_platform_by_host() {
        assert_eq!(platform_of("https://facebook.com/gravecord"), Some(0));
        assert_eq!(platform_of("https://www.youtube.com/x"), Some(7));
        assert_eq!(platform_of("https://artist.bandcamp.com/"), Some(10));
        assert_eq!(platform_of("https://example.com/"), None);
        assert_eq!(platform_of(""), None);
    }

    #[test]
    fn the_box_shows_what_follows_the_platform_address() {
        assert_eq!(
            link_tail("https://facebook.com/gravecord", Some(0)),
            "gravecord"
        );
        assert_eq!(
            link_tail("https://example.com/a", None),
            "https://example.com/a"
        );
    }

    #[test]
    fn a_first_address_gets_its_scheme_and_files_are_refused() {
        assert_eq!(
            typed_link("", "twitter.com/x").as_deref(),
            Some("https://twitter.com/x")
        );
        assert_eq!(typed_link("", "h").as_deref(), Some("h"));
        assert_eq!(
            typed_link("https://a.com/", "https://a.com/b").as_deref(),
            Some("https://a.com/b")
        );
        assert!(typed_link("https://a.com/", "https://a.com/setup.exe").is_none());
    }

    #[test]
    fn empty_link_rows_are_not_sent() {
        let meta = Meta {
            social: json!(["https://facebook.com/a", "", "https://twitter.com/"]),
            ..Meta::default()
        };
        assert_eq!(
            meta.patch_body()["social_links"],
            json!(["https://facebook.com/a", "https://twitter.com/"])
        );
    }

    #[test]
    fn reasons_need_ten_to_one_hundred_twenty_eight_characters() {
        assert!(reason_ready(
            &json!({"reason": "We hold weekly sessions", "emoji_name": "x"})
        ));
        assert!(!reason_ready(
            &json!({"reason": "short", "emoji_name": null})
        ));
        assert!(reason_empty(&json!({"reason": "", "emoji_name": null})));
    }

    #[test]
    fn publishing_waits_for_the_four_requirements() {
        let mut meta = Meta {
            about: json!("a".repeat(300)),
            reasons: json!([
                {"reason": "We hold weekly sessions", "emoji_name": "x"},
                {"reason": "Tips for keeping plants", "emoji_name": "y"},
                {"reason": "", "emoji_name": null},
                {"reason": "", "emoji_name": null},
            ]),
            ..Meta::default()
        };
        let rows = requirements(true, true, &meta);
        assert!(rows.iter().all(|(_, met)| *met));
        meta.about = json!("short");
        assert!(!requirements(true, true, &meta)[2].1);
        assert!(!requirements(false, true, &meta)[0].1);
    }
}
