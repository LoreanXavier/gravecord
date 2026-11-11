//! What a bot or an app shows under a message: buttons, selects, sections, text, thumbnails, galleries, files,
//! separators and containers (the component renderers of module 232430, the visual state of 911408).
//!
//! Every value is read from the pinned stylesheet; the class it comes from is named beside it. There is no official
//! golden of a components message, so nothing here has a parity id yet.
//!
//! * Button (`button_9OQtot`, size small): 1px border, radius 8, 14/16 medium, 32 high, 60 wide at least, content
//!   padding `calc(4 - 1)` by `calc(12 - 1)`.
//! * Select (`select_VXPAMp`, `select_uFkqU2`): 400 wide, 40 high at least, radius 8 like the other design-system
//!   select, a list under it that shows five options.
//! * Section (`section_5m-AKz`): text children left, the accessory right at no more than half the width less 12.
//! * Container (`container_rc97-L`): `--background-surface-high`, 1px `--border-subtle`, radius 8, padding 16, gap 8,
//!   a 4px accent bar on the start side.

use super::keyboard::{Layer, focusable, outside};
use super::motion::{self, Transitions as _};
use super::{Root, color, icon};
use crate::editor::Editor;
use dc_state::components::{
    self as model, ButtonStyle, Component, ComponentEmoji, EntityKind, GalleryItem, Media, Select,
};
use dc_state::interactions::{Chosen, Kind as InteractionKind, State as InteractionState};
use dc_state::{Id, Message};
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, Entity, FontWeight, IntoElement,
    SharedString, Subscription, deferred, div, img, prelude::*, px, relative,
};
use std::time::Duration;

/// The select whose list is open.
pub(super) struct OpenSelect {
    pub message: Id,
    pub component: String,
    /// What is picked while a multi select is open; applied when it closes.
    pub draft: Vec<(String, String)>,
    /// The values the select held when it opened.
    pub initial: Vec<String>,
    /// The search field of an entity select.
    pub search: Option<Entity<Editor>>,
    pub _event: Option<Subscription>,
}

#[derive(Default)]
pub(super) struct State {
    pub open: Option<OpenSelect>,
}

/// A thing of the entity lists: its value (an id), its label, and what picture goes with it.
#[derive(Clone)]
pub(super) struct Entity_ {
    pub value: String,
    pub label: String,
    pub avatar: Option<(String, String)>,
}

pub(super) fn wanted(message: &Message) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for component in &message.extra.components {
        collect_wanted(message.id, component, &mut out);
    }
    out
}

pub(super) fn media_key(message: Id, component: &str, index: usize) -> String {
    format!("comp:{message}:{component}:{index}")
}

fn media_url(media: &Media, width: f64, height: f64) -> String {
    let base = media.source();
    let joiner = if base.contains('?') { '&' } else { '?' };
    format!(
        "{base}{joiner}format=webp&width={}&height={}",
        width.round() as u32,
        height.round() as u32
    )
}

fn emoji_wanted(emoji: &ComponentEmoji, out: &mut Vec<(String, String)>) {
    if let Some(id) = emoji.id {
        out.push((
            format!("custom:{id}"),
            format!("https://cdn.discordapp.com/emojis/{id}.webp?size=96"),
        ));
    } else if let Some(name) = &emoji.name
        && let Some(file) = dc_tokens::emoji_file(name)
    {
        out.push((
            format!("emoji:{name}"),
            format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN),
        ));
    }
}

fn collect_wanted(message: Id, component: &Component, out: &mut Vec<(String, String)>) {
    match component {
        Component::ActionRow { components, .. } | Component::Container { components, .. } => {
            for inner in components {
                collect_wanted(message, inner, out);
            }
        }
        Component::Section {
            components,
            accessory,
            ..
        } => {
            for inner in components {
                collect_wanted(message, inner, out);
            }
            collect_wanted(message, accessory, out);
        }
        Component::Button {
            emoji: Some(emoji), ..
        } => emoji_wanted(emoji, out),
        Component::Select(select) => {
            for option in &select.options {
                if let Some(emoji) = &option.emoji {
                    emoji_wanted(emoji, out);
                }
            }
        }
        Component::Thumbnail { id, media, .. } => {
            out.push((
                media_key(message, id, 0),
                media_url(media, THUMBNAIL * 2., THUMBNAIL * 2.),
            ));
        }
        Component::MediaGallery { id, items } => {
            for (index, item) in items.iter().enumerate() {
                let (width, height) = tile_size(items.len(), index, &item.media);
                out.push((
                    media_key(message, id, index),
                    media_url(&item.media, width * 2., height * 2.),
                ));
            }
        }
        _ => {}
    }
}

/// Thumbnail: 85 by 85 (`custom-...` of module 232430's `nj`).
const THUMBNAIL: f64 = 85.;
/// The widest a V2 message gets: `min(600px, 100%)` (`container_uEFz2z` with the V2 modifier).
const V2_WIDTH: f32 = 600.;

/// The size a gallery picture is requested at: the largest tile of the dump (326 of 494), square.
fn tile_size(_count: usize, _index: usize, _media: &Media) -> (f64, f64) {
    (330., 330.)
}

/// The three dots of a pending interaction (`594808`): 7px circles 1.75 apart whose radius runs 0.8 to 1 of 3.5 and
/// whose opacity runs 0.3 to 1 on a triangle wave, 1200ms a cycle, each dot a quarter cycle behind the one before.
pub(super) fn dots(key: &str, tint: &'static str) -> AnyElement {
    let dot = |index: usize| {
        let name = SharedString::from(format!("{key}-dot-{index}"));
        div()
            .size(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .rounded_full()
                    .bg(color(tint))
                    .size(px(7.))
                    .with_animation(
                        name,
                        Animation::new(Duration::from_millis(1200)).repeat(),
                        move |element, delta| {
                            let cycle = (delta - 0.25 * index as f32).rem_euclid(1.);
                            let wave = 1. - (2. * cycle - 1.).abs();
                            let size = 7. * (0.8 + 0.2 * wave);
                            element.size(px(size)).opacity(0.3 + 0.7 * wave)
                        },
                    ),
            )
    };
    div()
        .flex()
        .items_center()
        .gap(px(1.75))
        .child(dot(0))
        .child(dot(1))
        .child(dot(2))
        .into_any_element()
}

/// What a button looks like in each state: background, hover, pressed, border, text.
struct Look {
    background: &'static str,
    hover: &'static str,
    active: &'static str,
    border: &'static str,
    text: &'static str,
}

fn look(style: ButtonStyle) -> Look {
    match style {
        // `colorBrand`: primary and premium.
        ButtonStyle::Primary | ButtonStyle::Premium => Look {
            background: "--control-primary-background-default",
            hover: "--control-primary-background-hover",
            active: "--control-primary-background-active",
            border: "--control-primary-border-default",
            text: "--control-primary-text-default",
        },
        // `colorGreen`.
        ButtonStyle::Success => Look {
            background: "--control-connected-background-default",
            hover: "--control-connected-background-hover",
            active: "--control-connected-background-active",
            border: "--control-connected-border-default",
            text: "--white",
        },
        // `colorRed`.
        ButtonStyle::Destructive => Look {
            background: "--control-critical-primary-background-default",
            hover: "--control-critical-primary-background-hover",
            active: "--control-critical-primary-background-active",
            border: "--control-critical-primary-border-default",
            text: "--white",
        },
        // Secondary, and a link button (which is a secondary one with a launch icon).
        ButtonStyle::Secondary | ButtonStyle::Link => Look {
            background: "--control-secondary-background-default",
            hover: "--control-secondary-background-hover",
            active: "--control-secondary-background-active",
            border: "--border-muted",
            text: "--control-secondary-text-default",
        },
    }
}

/// The state a component draws in (module 911408).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Visual {
    Normal,
    Loading,
    Disabled,
}

impl Root {
    /// The components of a message, or nothing.
    pub(super) fn message_components(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let components = &message.extra.components;
        if components.is_empty() {
            return None;
        }
        let v2 = message.flags & model::FLAG_IS_COMPONENTS_V2 != 0;
        let parts: Vec<AnyElement> = components
            .iter()
            .map(|component| self.component(message, component, false, cx))
            .collect();
        Some(
            div()
                .flex()
                .flex_col()
                .w_full()
                .gap(px(if v2 { 8. } else { 6. }))
                .when(v2, |this| this.max_w(px(V2_WIDTH)))
                .children(parts)
                .into_any_element(),
        )
    }

    /// How a component of this message draws: the pressed one loads, every other control waits.
    fn visual(&self, message: &Message, component: &Component) -> Visual {
        let Some(interaction) = self.stores.interactions.for_message(message.id) else {
            return Visual::Normal;
        };
        if interaction.state == InteractionState::Failed {
            return Visual::Normal;
        }
        if let InteractionKind::Component { component_id } = &interaction.kind
            && component_id == component.id()
        {
            return Visual::Loading;
        }
        if component.is_interactive() {
            Visual::Disabled
        } else {
            Visual::Normal
        }
    }

    /// The error text a failed press leaves under the row that holds the pressed component.
    fn failed_under(&self, message: &Message, holds: &[&Component]) -> Option<AnyElement> {
        let interaction = self.stores.interactions.for_message(message.id)?;
        if interaction.state != InteractionState::Failed {
            return None;
        }
        let InteractionKind::Component { component_id } = &interaction.kind else {
            return None;
        };
        holds
            .iter()
            .any(|component| component.contains(component_id))
            .then(|| {
                super::interaction_flow::failure_line(
                    interaction.error_message.as_deref(),
                    interaction.reason_code,
                )
            })
    }

    fn component(
        &self,
        message: &Message,
        component: &Component,
        inside_container: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match component {
            Component::ActionRow { components, .. } => {
                let held: Vec<&Component> = components.iter().collect();
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div().flex().flex_wrap().gap(px(8.)).children(
                            components
                                .iter()
                                .map(|inner| self.component(message, inner, inside_container, cx)),
                        ),
                    )
                    .children(
                        self.failed_under(message, &held)
                            .map(|line| div().my(px(4.)).child(line)),
                    )
                    .into_any_element()
            }
            Component::Button {
                id,
                custom_id,
                style,
                label,
                emoji,
                url,
                disabled,
                ..
            } => self.button(
                message,
                component,
                id,
                custom_id.as_deref(),
                *style,
                label.as_deref(),
                emoji.as_ref(),
                url.as_deref(),
                *disabled,
                cx,
            ),
            Component::Select(select) => self.component_select(message, component, select, cx),
            Component::TextInput(_) | Component::Label { .. } => {
                // These belong to a modal; in a message the official renderer shows its placeholder.
                self.unsupported()
            }
            Component::Section {
                components,
                accessory,
                ..
            } => {
                let held: Vec<&Component> = std::iter::once(accessory.as_ref()).collect();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .justify_between()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .children(components.iter().map(|inner| {
                                        self.component(message, inner, inside_container, cx)
                                    })),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .min_w(px(0.))
                                    .max_w(relative(0.5))
                                    .child(self.component(
                                        message,
                                        accessory,
                                        inside_container,
                                        cx,
                                    )),
                            ),
                    )
                    .children(self.failed_under(message, &held))
                    .into_any_element()
            }
            Component::TextDisplay { id, content } => {
                self.text_display(message, id, content, inside_container)
            }
            Component::Thumbnail {
                id,
                media: _,
                description: _,
                spoiler: _,
            } => self.picture(message, id, 0, THUMBNAIL as f32, THUMBNAIL as f32, 8.),
            Component::MediaGallery { id, items } => {
                self.gallery(message, id, items, inside_container)
            }
            Component::File {
                file, name, size, ..
            } => self.file_component(message, file, name.as_deref(), *size, cx),
            Component::Separator { divider, large, .. } => div()
                .w_full()
                .when(*large, |this| this.my(px(8.)))
                .when(*divider, |this| {
                    this.border_b_1()
                        .border_color(color("--background-mod-strong"))
                })
                .into_any_element(),
            Component::Container {
                accent_color,
                components,
                ..
            } => {
                let accent = *accent_color;
                div()
                    .relative()
                    .my(px(2.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .p(px(16.))
                    .overflow_hidden()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .when_some(accent, |this, accent| {
                        this.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .h_full()
                                .w(px(4.))
                                .bg(gpui::rgb(accent)),
                        )
                    })
                    .children(
                        components
                            .iter()
                            .map(|inner| self.component(message, inner, true, cx)),
                    )
                    .into_any_element()
            }
        }
    }

    /// `nJ`: what a component the client does not know is drawn as.
    fn unsupported(&self) -> AnyElement {
        div()
            .text_size(px(14.))
            .line_height(px(18.))
            .text_color(color("--text-muted"))
            .child("Please update Discord to view this message")
            .into_any_element()
    }

    fn text_display(
        &self,
        message: &Message,
        id: &str,
        content: &str,
        inside_container: bool,
    ) -> AnyElement {
        let key = (
            message.id,
            40_000 + id.bytes().map(u32::from).sum::<u32>() % 5000 * 16,
        );
        if inside_container {
            // `reduced`: 14/19.25 text inside a container; a heading is (## measured, # assumed the same) 20/27.5 bold with 8 below it
            // (dump of v2-container-p0: the text display is 27.5 + 8 + 19.25 tall).
            let mut parts: Vec<AnyElement> = Vec::new();
            let mut rest = String::new();
            let mut lines = content.lines().peekable();
            while let Some(line) = lines.next() {
                if let Some(heading) = line.strip_prefix("## ").or_else(|| line.strip_prefix("# "))
                {
                    if !rest.is_empty() {
                        parts.push(self.embed_text(&std::mem::take(&mut rest), key));
                    }
                    parts.push(
                        div()
                            .text_size(px(20.))
                            .line_height(px(27.5))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .when(lines.peek().is_some(), |this| this.mb(px(8.)))
                            .child(heading.to_owned())
                            .into_any_element(),
                    );
                } else {
                    if !rest.is_empty() {
                        rest.push('\n');
                    }
                    rest.push_str(line);
                }
            }
            if !rest.is_empty() {
                parts.push(self.embed_text(&rest, key));
            }
            return div().flex().flex_col().children(parts).into_any_element();
        }
        let mut shown = message.clone();
        shown.content = content.to_owned();
        self.content(&shown).into_any_element()
    }

    fn picture(
        &self,
        message: &Message,
        id: &str,
        index: usize,
        width: f32,
        height: f32,
        radius: f32,
    ) -> AnyElement {
        let shown = self.images.get(&media_key(message.id, id, index)).cloned();
        div()
            .flex_none()
            .w(px(width))
            .h(px(height))
            .rounded(px(radius))
            .overflow_hidden()
            .bg(color("--background-base-low"))
            .when_some(shown, |this, image| {
                this.child(
                    img(image)
                        .w(px(width))
                        .h(px(height))
                        .object_fit(gpui::ObjectFit::Cover),
                )
            })
            .into_any_element()
    }

    /// The media gallery (`visualMediaItemContainer_idVBIj` in a V2 message): square tiles with a 4px gap that fill the
    /// row (dump: 1 = 256, 2 = (W-4)/2 each, 3 = a large (W-4)*2/3 square left and two (W-4)/3 stacked right).
    fn gallery(
        &self,
        message: &Message,
        id: &str,
        items: &[GalleryItem],
        inside_container: bool,
    ) -> AnyElement {
        let width = if inside_container { 494. } else { 516. };
        let tile = |index: usize, size: f32| self.picture(message, id, index, size, size, 2.);
        let count = items.len();
        let body = match count {
            1 => div().child(tile(0, 256f32.min(width))),
            2 => div()
                .flex()
                .gap(px(4.))
                .child(tile(0, (width - 4.) / 2.))
                .child(tile(1, (width - 4.) / 2.)),
            3 => {
                let small = (width - 4.) / 3.;
                div().flex().gap(px(4.)).child(tile(0, small * 2.)).child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(tile(1, (small * 2. - 4.) / 2.))
                        .child(tile(2, (small * 2. - 4.) / 2.)),
                )
            }
            _ => div()
                .flex()
                .flex_wrap()
                .gap(px(4.))
                .children((0..count).map(|index| tile(index, (width - 4.) / 2.))),
        };
        div()
            .w_full()
            .overflow_hidden()
            .rounded(px(8.))
            .child(body)
            .into_any_element()
    }

    /// The File component: the plaintext card's frame (`container_BOOBu-`), a link-colored name and the size under it.
    fn file_component(
        &self,
        message: &Message,
        file: &Media,
        name: Option<&str>,
        size: Option<u64>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = name
            .map(str::to_owned)
            .unwrap_or_else(|| file.url.rsplit('/').next().unwrap_or("file").to_owned());
        let url = file.url.clone();
        let label = title.clone();
        // Dump (file_-9z-XO): 432 x 74, padding 16, border 1 --border-subtle, radius 8, icon 30 x 40 with 8 after it,
        // name 16/20 link colour, size 12/16 muted.
        div()
            .id(SharedString::from(format!(
                "comp-file-{}-{}",
                message.id, title
            )))
            .w(px(432.))
            .max_w_full()
            .p(px(16.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_none()
                    .w(px(30.))
                    .h(px(40.))
                    .rounded(px(3.))
                    .border_1()
                    .border_color(color("--text-muted"))
                    .opacity(0.8),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-link"))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(title),
                    )
                    .when_some(size, |this, size| {
                        this.child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-muted"))
                                .child(super::files::format_size(size)),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.follow_link(&url, &label, cx)))
            .into_any_element()
    }

    /// An emoji of a button or an option: 20px, from the images the message asked for.
    fn component_emoji(&self, emoji: &ComponentEmoji, size: f32) -> Option<AnyElement> {
        let key = match (&emoji.id, &emoji.name) {
            (Some(id), _) => format!("custom:{id}"),
            (None, Some(name)) => format!("emoji:{name}"),
            _ => return None,
        };
        let shown = self.images.get(&key).cloned();
        Some(
            div()
                .flex_none()
                .size(px(size))
                .when_some(shown, |this, image| this.child(img(image).size(px(size))))
                .into_any_element(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn button(
        &self,
        message: &Message,
        component: &Component,
        id: &str,
        custom_id: Option<&str>,
        style: ButtonStyle,
        label: Option<&str>,
        emoji: Option<&ComponentEmoji>,
        url: Option<&str>,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let visual = self.visual(message, component);
        let colors = look(style);
        let link = style == ButtonStyle::Link;
        let inert = disabled || visual != Visual::Normal && !link || (link && url.is_none());
        let key = format!("comp-{}-{id}", message.id);
        let handle = self.stop(&key, cx);
        let (message_id, component_id) = (message.id, id.to_owned());
        let target = url.map(str::to_owned);
        let text = label.unwrap_or_default().to_owned();
        let _ = custom_id;
        let loading = visual == Visual::Loading;
        let content = div()
            .flex()
            .items_center()
            .justify_center()
            .m_auto()
            .min_w(px(32.))
            .when(loading, |this| this.invisible())
            .when_some(
                emoji.and_then(|emoji| self.component_emoji(emoji, 20.)),
                |this, shown| {
                    this.child(
                        div()
                            .when(label.is_some(), |this| this.mr(px(4.)))
                            .child(shown),
                    )
                },
            )
            .when(label.is_some(), |this| {
                this.child(
                    div()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(text.clone()),
                )
            })
            .when(link, |this| {
                this.child(
                    div()
                        .ml(px(8.))
                        .child(icon("access_launch", 16., colors.text)),
                )
            });
        let element = div()
            .id(SharedString::from(key))
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .h(px(32.))
            .min_w(px(60.))
            .px(px(11.))
            .py(px(3.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(colors.border))
            .bg(color(colors.background))
            .text_color(color(colors.text))
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .rule(motion::Rule::Button)
            .child(content)
            .when(loading, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(dots(&format!("btn-{}-{id}", message.id), colors.text)),
                )
            });
        let element = if inert {
            element
                .when(disabled || visual == Visual::Disabled, |this| {
                    this.opacity(0.5)
                })
                .cursor_not_allowed()
        } else {
            let hover = colors.hover;
            let active = colors.active;
            element
                .cursor_pointer()
                .hover(move |style| style.bg(color(hover)))
                .active(move |style| style.bg(color(active)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if link {
                        if let Some(url) = &target {
                            this.follow_link(url, "", cx);
                        }
                    } else {
                        this.press_button(message_id, &component_id, cx);
                    }
                }))
        };
        focusable(
            element,
            &handle,
            !inert && self.modal_open() == 0,
            self.keyboard_mode(),
        )
        .into_any_element()
    }

    /// What a select shows as its value: what was chosen, else what the payload preselected.
    pub(super) fn select_values(&self, container: &str, select: &Select) -> Vec<(String, String)> {
        match self.stores.interactions.value(container, &select.id) {
            Some(Chosen::Strings(values)) => values
                .iter()
                .map(|value| (value.clone(), option_label(select, value)))
                .collect(),
            Some(Chosen::Entities(values)) => values.clone(),
            Some(Chosen::Text(_)) => Vec::new(),
            None if select.kind == model::STRING_SELECT => select
                .options
                .iter()
                .filter(|option| option.default)
                .map(|option| (option.value.clone(), option.label.clone()))
                .collect(),
            None => select
                .default_values
                .iter()
                .map(|known| {
                    let value = known.id.to_string();
                    let label = self.entity_label(known.kind, known.id);
                    (value, label)
                })
                .collect(),
        }
    }

    fn component_select(
        &self,
        message: &Message,
        component: &Component,
        select: &Select,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let visual = self.visual(message, component);
        let container = message.id.to_string();
        let shown = self.select_values(&container, select);
        let open = self
            .components
            .open
            .as_ref()
            .filter(|open| open.message == message.id && open.component == select.id);
        let list_open = open.is_some();
        let multi = select.max_values > 1;
        let shown = match open {
            Some(open) if multi => open.draft.clone(),
            _ => shown,
        };
        let inert = select.disabled || visual != Visual::Normal;
        let key = format!("comp-{}-{}", message.id, select.id);
        let handle = self.stop(&key, cx);
        let (message_id, component_id) = (message.id, select.id.clone());
        let placeholder = select
            .placeholder
            .clone()
            .unwrap_or_else(|| "Make a selection".to_owned());
        let value: AnyElement = if shown.is_empty() {
            div()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_color(color("--text-subtle"))
                .child(placeholder)
                .into_any_element()
        } else if multi {
            div()
                .flex()
                .flex_wrap()
                .gap(px(4.))
                .children(shown.iter().map(|(_, label)| {
                    div()
                        .flex()
                        .items_center()
                        .h(px(20.))
                        .px(px(8.))
                        .py(px(6.))
                        .rounded(px(3.))
                        .max_w(px(230.))
                        .bg(color("--background-base-low"))
                        .text_color(color("--interactive-text-active"))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(label.clone())
                }))
                .into_any_element()
        } else {
            div()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(shown[0].1.clone())
                .into_any_element()
        };
        let control = div()
            .id(SharedString::from(key))
            .flex()
            .items_center()
            .min_h(px(40.))
            .w_full()
            .py(px(8.))
            .pl(px(12.))
            .pr(px(8.))
            .gap(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(if list_open {
                color("--text-brand")
            } else {
                color("--input-border-default")
            })
            .bg(color("--input-background-default"))
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-default"))
            .when(select.disabled, |this| this.opacity(0.6))
            .child(div().flex_1().min_w(px(0.)).child(value))
            .child(icon("settings_select_chevron", 20., "--text-default").when(
                list_open,
                |this| {
                    this.with_transformation(gpui::Transformation::rotate(gpui::percentage(0.5)))
                },
            ));
        let control = if inert {
            control.cursor_not_allowed()
        } else {
            control
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.toggle_select(message_id, &component_id, window, cx);
                }))
        };
        let control = focusable(
            control,
            &handle,
            !inert && self.modal_open() == 0,
            self.keyboard_mode(),
        );
        let held = [component];
        div()
            .flex()
            .flex_col()
            .w(px(400.))
            .max_w_full()
            .child(
                div()
                    .relative()
                    .w_full()
                    .child(control)
                    .when_some(open, |this, open| {
                        this.child(self.select_list(message, select, open, cx))
                    }),
            )
            .children(self.failed_under(message, &held))
            .into_any_element()
    }

    /// The list under an open select (`popout_uFkqU2`): five options in view, the rest scrolls.
    fn select_list(
        &self,
        message: &Message,
        select: &Select,
        open: &OpenSelect,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let multi = select.max_values > 1;
        let query = open
            .search
            .as_ref()
            .map(|search| search.read(cx).text().to_lowercase())
            .unwrap_or_default();
        let chosen: Vec<&str> = if multi {
            open.draft.iter().map(|(value, _)| value.as_str()).collect()
        } else {
            self.stores
                .interactions
                .value(&message.id.to_string(), &select.id)
                .map(|held| match held {
                    Chosen::Strings(values) => values.iter().map(String::as_str).collect(),
                    Chosen::Entities(values) => {
                        values.iter().map(|(value, _)| value.as_str()).collect()
                    }
                    Chosen::Text(_) => Vec::new(),
                })
                .unwrap_or_default()
        };
        // (value, label, description, emoji, avatar)
        struct Row {
            value: String,
            label: String,
            description: Option<String>,
            emoji: Option<ComponentEmoji>,
            avatar: Option<String>,
        }
        let rows: Vec<Row> = if select.kind == model::STRING_SELECT {
            select
                .options
                .iter()
                .map(|option| Row {
                    value: option.value.clone(),
                    label: option.label.clone(),
                    description: option.description.clone(),
                    emoji: option.emoji.clone(),
                    avatar: None,
                })
                .collect()
        } else {
            self.entity_options(message.channel_id, select, &query)
                .into_iter()
                .map(|entity| Row {
                    value: entity.value,
                    label: entity.label,
                    description: None,
                    emoji: None,
                    avatar: entity.avatar.map(|(key, _)| key),
                })
                .collect()
        };
        let (message_id, component_id) = (message.id, select.id.clone());
        let items: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let picked = chosen.contains(&row.value.as_str());
                let (value, label) = (row.value.clone(), row.label.clone());
                let component = component_id.clone();
                div()
                    .id(SharedString::from(format!(
                        "comp-opt-{message_id}-{component_id}-{index}"
                    )))
                    .flex_none()
                    .flex()
                    .items_center()
                    .p(px(8.))
                    .text_size(px(16.))
                    .cursor_pointer()
                    .rule(motion::Rule::ListBoxItem)
                    .when(picked, |this| {
                        this.bg(color("--interactive-background-selected"))
                    })
                    .hover(|style| {
                        style
                            .bg(color("--interactive-background-hover"))
                            .text_color(color("--interactive-text-hover"))
                    })
                    .when(multi, |this| {
                        this.child(
                            div()
                                .flex_none()
                                .mr(px(8.))
                                .size(px(20.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(color("--interactive-text-default"))
                                .when(picked, |this| {
                                    this.bg(color("--brand-500")).child(icon(
                                        "checkbox_check",
                                        18.,
                                        "--white",
                                    ))
                                }),
                        )
                    })
                    .when_some(
                        row.emoji
                            .as_ref()
                            .and_then(|emoji| self.component_emoji(emoji, 16.)),
                        |this, shown| this.child(div().mr(px(8.)).child(shown)),
                    )
                    .when_some(
                        row.avatar
                            .as_ref()
                            .and_then(|key| self.images.get(key).cloned()),
                        |this, image| {
                            this.child(
                                div()
                                    .mr(px(8.))
                                    .flex_none()
                                    .size(px(24.))
                                    .rounded_full()
                                    .overflow_hidden()
                                    .child(img(image).size(px(24.))),
                            )
                        },
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .child(
                                div()
                                    .mb(px(4.))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_color(color(if picked {
                                        "--interactive-text-active"
                                    } else {
                                        "--text-strong"
                                    }))
                                    .child(row.label.clone()),
                            )
                            .when_some(row.description.clone(), |this, description| {
                                this.child(
                                    div()
                                        .text_color(color("--interactive-text-default"))
                                        .child(description),
                                )
                            }),
                    )
                    .when(picked && !multi, |this| {
                        this.child(icon("select_check", 20., "--brand-500"))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pick_option(message_id, &component, &value, &label, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        let searchable = open.search.is_some();
        deferred(
            outside(
                div().id(SharedString::from(format!(
                    "comp-list-{message_id}-{component_id}"
                ))),
                Layer::ComponentSelect,
                cx,
            )
            .occlude()
            .absolute()
            .top(px(48.))
            .left_0()
            .right_0()
            .rounded_b(px(4.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .when_some(
                open.search.clone().filter(|_| searchable),
                |this, search| {
                    this.child(
                        div()
                            .px(px(12.))
                            .py(px(10.))
                            .border_b_1()
                            .border_color(color("--border-subtle"))
                            .child(search),
                    )
                },
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "comp-scroll-{message_id}-{component_id}"
                    )))
                    .max_h(px(5. * 38.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .when(items.is_empty(), |this| {
                        this.child(
                            div()
                                .p(px(12.))
                                .text_size(px(14.))
                                .text_color(color("--text-muted"))
                                .child("No results"),
                        )
                    })
                    .children(items),
            ),
        )
        .with_priority(2)
        .into_any_element()
    }

    /// The name an id of an entity select is drawn with.
    pub(super) fn entity_label(&self, kind: EntityKind, id: Id) -> String {
        match kind {
            EntityKind::User => self
                .stores
                .users
                .get(&id)
                .map(|user| user.display_name().to_owned())
                .unwrap_or_else(|| id.to_string()),
            EntityKind::Role => self
                .guild
                .and_then(|guild| self.stores.guilds.iter().find(|known| known.id == guild))
                .and_then(|guild| guild.roles.get(&id))
                .map(|name| format!("@{name}"))
                .unwrap_or_else(|| id.to_string()),
            EntityKind::Channel => self
                .stores
                .guilds
                .iter()
                .flat_map(|guild| &guild.channels)
                .find(|channel| channel.id == id)
                .map(|channel| format!("# {}", channel.name))
                .unwrap_or_else(|| id.to_string()),
        }
    }

    /// What an entity select lists: at most fifteen, filtered by what was typed.
    pub(super) fn entity_options(&self, channel: Id, select: &Select, query: &str) -> Vec<Entity_> {
        let guild = self.stores.guilds.iter().find(|known| {
            Some(known.id) == self.guild || known.channels.iter().any(|c| c.id == channel)
        });
        let mut out: Vec<Entity_> = Vec::new();
        let wants_users = matches!(select.kind, model::USER_SELECT | model::MENTIONABLE_SELECT);
        let wants_roles = matches!(select.kind, model::ROLE_SELECT | model::MENTIONABLE_SELECT);
        if wants_users {
            if let Some(guild) = guild {
                for member in self.stores.members.of(guild.id) {
                    out.push(self.user_entity(&member.user, member.nick.as_deref()));
                }
            }
            if out.is_empty() {
                let mut seen = std::collections::HashSet::new();
                for known in self.stores.messages.get(&channel).into_iter().flatten() {
                    if seen.insert(known.author.id) {
                        out.push(self.user_entity(&known.author, None));
                    }
                }
            }
        }
        if wants_roles && let Some(guild) = guild {
            let mut roles: Vec<(&Id, &String)> = guild
                .roles
                .iter()
                .filter(|(id, _)| **id != guild.id)
                .collect();
            roles.sort_by(|a, b| a.1.cmp(b.1));
            for (id, name) in roles {
                out.push(Entity_ {
                    value: id.to_string(),
                    label: format!("@{name}"),
                    avatar: None,
                });
            }
        }
        if select.kind == model::CHANNEL_SELECT
            && let Some(guild) = guild
        {
            for channel in &guild.channels {
                let raw = match channel.kind {
                    dc_state::ChannelKind::Text => 0,
                    dc_state::ChannelKind::Voice => 2,
                    dc_state::ChannelKind::Category => 4,
                    dc_state::ChannelKind::Other(other) => other,
                };
                if select.channel_types.is_empty() || select.channel_types.contains(&raw) {
                    out.push(Entity_ {
                        value: channel.id.to_string(),
                        label: format!("# {}", channel.name),
                        avatar: None,
                    });
                }
            }
        }
        let query = query.trim().trim_start_matches(['@', '#']).trim();
        out.retain(|entity| {
            query.is_empty()
                || entity
                    .label
                    .trim_start_matches(['@', '#', ' '])
                    .to_lowercase()
                    .contains(query)
        });
        out.truncate(15);
        out
    }

    fn user_entity(&self, user: &dc_state::User, nick: Option<&str>) -> Entity_ {
        let (key, url) = super::avatar_source(user);
        Entity_ {
            value: user.id.to_string(),
            label: nick.unwrap_or_else(|| user.display_name()).to_owned(),
            avatar: Some((key, url)),
        }
    }
}

/// The label of a string select's value.
fn option_label(select: &Select, value: &str) -> String {
    select
        .options
        .iter()
        .find(|option| option.value == value)
        .map(|option| option.label.clone())
        .unwrap_or_else(|| value.to_owned())
}
