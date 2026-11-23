//! Server Settings > Access > Discoverable, once the server passes the requirements: the listing form (module 989433
//! `A1` with `Aq`, `AJ`, `AZ`, `AQ`, `A$`, `AY`) and the agreement that comes before listing (module 883494).
//! Measured in `design/dom/settings/guild-access-elig-*`.

use super::server_discovery::{
    DEFAULT_SPLASH, Select, all_categories, popular_tags, primary_categories, splash_url,
};
use super::tooltip::{Side, Tip};
use super::{Root, color, icon};
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, img, prelude::*, px,
};

/// `AX`: the asterisk after a required label.
fn required() -> gpui::Div {
    div()
        .ml(px(4.))
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight(600.))
        .text_color(color("--text-feedback-critical"))
        .child("*")
}

/// `body-md/semibold` header of a section.
fn section_header(text: &'static str) -> gpui::Div {
    div()
        .mb(px(8.))
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(600.))
        .text_color(color("--text-strong"))
        .child(text)
}

/// `body-sm/medium` under it.
fn section_lead(text: &'static str) -> gpui::Div {
    div()
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-subtle"))
        .child(text)
}

/// `body-xs/semibold` subHeader of a field.
fn sub_header(text: &'static str, needed: bool) -> gpui::Div {
    div()
        .mb(px(4.))
        .flex()
        .items_start()
        .text_size(px(14.))
        .line_height(px(18.6562))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-muted"))
        .child(text)
        .when(needed, |this| this.child(required().mt(px(2.))))
}

/// `A0`.
fn rule() -> gpui::Div {
    div().h(px(1.)).w_full().bg(color("--border-subtle"))
}

/// A `secondary` small button (`size: "sm"`).
fn small_button(id: &'static str, text: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(32.))
        .px(px(12.))
        .rounded(px(8.))
        .bg(color("--control-secondary-background-default"))
        .hover(|style| style.bg(color("--control-secondary-background-hover")))
        .cursor_pointer()
        .flex()
        .flex_none()
        .items_center()
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(500.))
        .text_color(color("--control-secondary-text-default"))
        .child(text)
}

/// A removable chip: a tag, or a chosen subcategory.
fn chip(
    id: SharedString,
    text: String,
    on_remove: impl Fn(&mut Root, &mut Context<Root>) + 'static,
    cx: &mut Context<Root>,
) -> AnyElement {
    div()
        .id(id.clone())
        .h(px(32.))
        .flex_none()
        .pl(px(8.))
        .pr(px(4.))
        .rounded(px(4.))
        .bg(color("--background-mod-subtle"))
        .flex()
        .items_center()
        .child(
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(text),
        )
        .child(
            div()
                .id(SharedString::from(format!("{id}-remove")))
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .child(icon("close", 12., "--interactive-text-default"))
                .on_click(cx.listener(move |this, _, _, cx| on_remove(this, cx))),
        )
        .into_any_element()
}

impl Root {
    fn discovery_state(&self) -> Option<&super::server_discovery::Discovery> {
        self.server_settings.as_ref().map(|open| &open.discovery)
    }

    /// `A1`.
    pub(super) fn discovery_form(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(discovery) = self.discovery_state().filter(|state| state.loaded()) else {
            return Self::discovery_checking();
        };
        let needed =
            self.discovery_view(guild) == Some(super::server_discovery::View::EligibleDisabled);
        let primary_name = discovery
            .categories
            .iter()
            .find(|category| category.id == discovery.meta.primary)
            .map(|category| category.name.clone());
        let editor = discovery.description.clone();
        let focus = editor.clone();
        let name = self
            .stores
            .guild(guild)
            .map(|known| known.name.clone())
            .unwrap_or_default();
        let preview = self.discovery_preview(guild, &name);

        let profile = div()
            .flex()
            .gap(px(48.))
            .child(
                div()
                    .w(px(358.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(32.))
                    .child(
                        div()
                            .child(section_header("Profile"))
                            .child(section_lead("This is how your server will look in Discovery.")),
                    )
                    .child(
                        div()
                            .child(sub_header("Banner", false))
                            .child(section_lead(
                                "Choose your server's banner. The recommended minimum size is 1920x1080 and recommended aspect ratio is 16:9.",
                            ))
                            .child(
                                div()
                                    .mt(px(16.))
                                    .h(px(32.))
                                    .flex()
                                    .gap(px(8.))
                                    .child(
                                        small_button("discovery-upload", "Upload").on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.discovery_pick_splash(cx)
                                            }),
                                        ),
                                    )
                                    .when(discovery.bits.splash.is_some(), |this| {
                                        this.child(
                                            div()
                                                .id("discovery-remove-banner")
                                                .h(px(32.))
                                                .px(px(12.))
                                                .rounded(px(8.))
                                                .border_1()
                                                .border_color(color(
                                                    "--control-critical-secondary-border-default",
                                                ))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .text_size(px(14.))
                                                .line_height(px(18.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color(
                                                    "--control-critical-secondary-text-default",
                                                ))
                                                .child("Remove")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.discovery_remove_splash(cx)
                                                })),
                                        )
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .child(sub_header("Description", needed))
                            .child(section_lead(
                                "How did your server get started, and what makes your crew special?",
                            ))
                            .child(
                                div()
                                    .id("discovery-description")
                                    .mt(px(16.))
                                    .h(px(89.))
                                    .pt(px(12.))
                                    .px(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-default"))
                                    .bg(color("--input-background-default"))
                                    .cursor_text()
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        if let Some(focus) = focus.as_ref() {
                                            let handle =
                                                gpui::Focusable::focus_handle(focus.read(cx), cx);
                                            window.focus(&handle, cx);
                                        }
                                    }))
                                    .children(editor.map(|editor| div().w_full().child(editor))),
                            ),
                    ),
            )
            .child(preview);

        let category = div()
            .flex()
            .flex_col()
            .gap(px(32.))
            .child(
                div()
                    .child(section_header("Server Category"))
                    .child(section_lead(
                        "Help people find your server in search results",
                    )),
            )
            .child(
                div()
                    .child(sub_header("Category", needed))
                    .child(section_lead(
                        "Choose a subject area that best fits your server.",
                    ))
                    .child(div().mt(px(16.)).child(self.discovery_select(
                        Select::Category,
                        primary_name.map(|name| vec![name]).unwrap_or_default(),
                        discovery.meta.primary != 0,
                        cx,
                    ))),
            )
            .child(
                div()
                    .child(sub_header("Subcategories", false))
                    .child(section_lead(
                        "Get more specific, if you want. You can add up to 5.",
                    ))
                    .child(
                        div().mt(px(16.)).child(
                            self.discovery_select(
                                Select::Subcategories,
                                discovery
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
                                    .collect(),
                                false,
                                cx,
                            ),
                        ),
                    ),
            );

        let full = discovery.meta.keywords.len() >= super::server_discovery::MAX_TAGS;
        let tags = div()
            .flex()
            .flex_col()
            .gap(px(32.))
            .child(
                div()
                    .child(
                        div()
                            .mb(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(4.))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child("Tags")
                                    .when(needed, |this| {
                                        this.child(
                                            div()
                                                .font_weight(FontWeight(400.))
                                                .text_color(color("--text-feedback-critical"))
                                                .child("*"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child("In Server Discovery, you can search by name, description, and tags. You can add up to 10."),
                            ),
                    )
                    .child(self.discovery_tag_field(cx))
                    .children(discovery.tag_error.clone().map(|message| {
                        div()
                            .mt(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-feedback-critical"))
                            .child(message)
                    })),
            )
            .children({
                let suggestions: Vec<&'static str> = popular_tags(discovery.meta.primary)
                    .iter()
                    .copied()
                    .filter(|tag| !discovery.meta.keywords.iter().any(|known| known == tag))
                    .collect();
                (!suggestions.is_empty()).then(|| {
                    div()
                        .child(sub_header("Popular Tags", false).mb(px(0.)))
                        .child(
                            div()
                                .mt(px(16.))
                                .flex()
                                .flex_wrap()
                                .gap(px(8.))
                                .children(suggestions.into_iter().map(|tag| {
                                    let pill = div()
                                        .id(SharedString::from(format!("discovery-popular-{tag}")))
                                        .h(px(36.))
                                        .px(px(12.))
                                        .rounded_full()
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .flex()
                                        .flex_none()
                                        .items_center()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-subtle"))
                                        .child(tag);
                                    if full {
                                        // A disabled pill says why on hover (`tooltipText`, module 513521's string).
                                        self.with_tip(
                                            format!("discovery-popular-tip-{tag}"),
                                            Tip::text(dc_intl::t("Xx7XeB"), Side::Top),
                                            pill.opacity(0.5),
                                            cx,
                                        )
                                        .into_any_element()
                                    } else {
                                        pill.cursor_pointer()
                                            .hover(|style| {
                                                style.bg(color("--interactive-background-hover"))
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.discovery_add_tag(tag.to_owned(), cx)
                                            }))
                                            .into_any_element()
                                    }
                                })),
                        )
                })
            });

        let emoji_on = discovery.meta.emoji;
        let emoji = div()
            .max_w(px(612.))
            .child(section_header("Show Emoji/Sticker Info When Clicked"))
            .child(section_lead(
                "This allows info to appear when clicking on a custom emoji or sticker from your server in any chat. Users will be able to see and join your server via the information that is displayed.",
            ))
            .child(
                div().mt(px(16.)).flex().child(
                    div()
                        .id("discovery-emoji")
                        .h(px(32.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .when(emoji_on, |this| {
                            this.bg(color("--control-secondary-background-default"))
                                .text_color(color("--control-secondary-text-default"))
                        })
                        .when(!emoji_on, |this| {
                            this.bg(color("--control-connected-background-default"))
                                .text_color(gpui::white())
                        })
                        .child(if emoji_on {
                            "Disable Emoji/Sticker Information"
                        } else {
                            "Enable Emoji/Sticker Information"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.discovery_set_emoji(!emoji_on, cx)
                        })),
                ),
            );

        let language = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex_1()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Server Primary Language"),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child("Discord will prioritize this server in search to users who speak the selected language."),
                    ),
            )
            .child(self.discovery_language_field(cx));

        div()
            .flex()
            .flex_col()
            .gap(px(32.))
            .child(profile)
            .child(rule())
            .child(category)
            .child(rule())
            .child(tags)
            .child(rule())
            .child(emoji)
            .child(rule())
            .child(language)
            .child(rule())
            .child(self.access_invite_section(cx))
            .into_any_element()
    }

    /// `AH`: the card a server gets in Discovery.
    fn discovery_preview(&self, guild: Id, name: &str) -> AnyElement {
        let Some(discovery) = self.discovery_state() else {
            return div().into_any_element();
        };
        let splash = discovery.bits.splash.as_deref().map(|value| {
            if value.starts_with("data:") {
                value.to_owned()
            } else {
                splash_url(guild, value)
            }
        });
        let picture = splash
            .as_deref()
            .and_then(|url| self.images.get(url).cloned())
            .or_else(|| self.images.get(DEFAULT_SPLASH).cloned());
        let acronym = dc_state::Guild::acronym_of(name);
        div()
            .relative()
            .w(px(254.))
            .h(px(266.))
            .flex_none()
            .rounded(px(8.))
            .bg(color("--background-surface-high"))
            .shadow(vec![
                gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 1., 0.08),
                    offset: gpui::point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(1.),
                    inset: false,
                },
                gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: gpui::point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ])
            .child(
                div()
                    .h(px(112.))
                    .w_full()
                    .rounded_t(px(8.))
                    .overflow_hidden()
                    .children(picture.map(|picture| {
                        img(picture)
                            .w(px(254.))
                            .h(px(112.))
                            .object_fit(gpui::ObjectFit::Cover)
                    })),
            )
            .child(
                div()
                    .absolute()
                    .top(px(82.))
                    .left(px(12.))
                    .size(px(48.))
                    .rounded(px(14.))
                    .bg(gpui::rgba(0x97979f29))
                    .p(px(4.))
                    .child(
                        div()
                            .size(px(40.))
                            .rounded(px(10.))
                            .bg(color("--background-brand"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--white"))
                            .child(acronym),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(140.))
                    .left(px(12.))
                    .w(px(230.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(name.to_owned()),
                    )
                    .child(
                        div().h(px(78.)).child(
                            div()
                                .my(px(8.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .max_h(px(36.))
                                .overflow_hidden()
                                .child(discovery.bits.description.clone().unwrap_or_default()),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(div().size(px(8.)).rounded_full().bg(color("--text-muted")))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-default"))
                                    .child(format!("{} Members", discovery.members)),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// `AU.A`'s field: the tags, then the box to type the next one in.
    fn discovery_tag_field(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(discovery) = self.discovery_state() else {
            return div().into_any_element();
        };
        let input = discovery.tag_input.clone();
        let focus = input.clone();
        let empty = discovery.meta.keywords.is_empty();
        div()
            .id("discovery-tags")
            .min_h(px(42.))
            .p(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--input-border-default"))
            .bg(color("--input-background-default"))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.))
            .cursor_text()
            .on_click(cx.listener(move |_, _, window, cx| {
                if let Some(focus) = focus.as_ref() {
                    let handle = gpui::Focusable::focus_handle(focus.read(cx), cx);
                    window.focus(&handle, cx);
                }
            }))
            .children(discovery.meta.keywords.iter().map(|tag| {
                let removed = tag.clone();
                chip(
                    SharedString::from(format!("discovery-tag-{tag}")),
                    tag.clone(),
                    move |this, cx| this.discovery_remove_tag(&removed, cx),
                    cx,
                )
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(120.))
                    .h(px(32.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .child(div().w_full().children(input))
                    .when(empty, |this| {
                        this.child(
                            div()
                                .absolute()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--input-placeholder-text-default"))
                                .child(""),
                        )
                    }),
            )
            .into_any_element()
    }

    /// One `e2.Z` field: what is chosen, the clear button, the chevron.
    pub(super) fn discovery_select(
        &self,
        which: Select,
        chosen: Vec<String>,
        clearable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let opened = self
            .discovery_state()
            .is_some_and(|state| state.open == Some(which));
        let key = which.key();
        let multiple = which == Select::Subcategories;
        let none = chosen.is_empty();
        let filter = self
            .discovery_state()
            .and_then(|state| state.filter.clone());
        div()
            .id(key)
            .relative()
            .child(self.anchor_probe(key))
            .min_h(px(40.))
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
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(4.))
                    .when(none && !opened, |this| {
                        this.child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--input-placeholder-text-default"))
                                .child("Select..."),
                        )
                    })
                    .children(
                        chosen
                            .into_iter()
                            .enumerate()
                            .filter(|_| multiple || !opened)
                            .map(|(index, name)| {
                                if multiple {
                                    let id = self.discovery_secondary_at(index);
                                    chip(
                                        SharedString::from(format!("discovery-sub-{index}")),
                                        name,
                                        move |this, cx| {
                                            if let Some(id) = id {
                                                this.discovery_toggle_secondary(id, cx);
                                            }
                                        },
                                        cx,
                                    )
                                } else {
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-strong"))
                                        .child(name)
                                        .into_any_element()
                                }
                            }),
                    )
                    .children(
                        opened
                            .then(|| filter.clone())
                            .flatten()
                            .map(|filter| div().flex_1().min_w(px(40.)).child(filter)),
                    ),
            )
            .when(clearable, |this| {
                this.child(
                    div()
                        .id(SharedString::from(format!("{key}-clear")))
                        .w(px(24.))
                        .h(px(38.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("circle_x_small", 16., "--interactive-text-default"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.discovery_set_primary(0, cx);
                        })),
                )
            })
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
                            .with_transformation(gpui::Transformation::rotate(gpui::percentage(
                                if opened { 0.5 } else { 0. },
                            ))),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.discovery_toggle_select(which, window, cx)
            }))
            .into_any_element()
    }

    /// Opening a select puts its text box in the field with the value in it, and scrolls the list to the value.
    fn discovery_toggle_select(
        &mut self,
        which: Select,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let Some(discovery) = self
            .server_settings
            .as_mut()
            .map(|open| &mut open.discovery)
        else {
            return;
        };
        if discovery.open == Some(which) {
            discovery.open = None;
            cx.notify();
            return;
        }
        discovery.open = Some(which);
        let value = if which == Select::Category {
            discovery
                .categories
                .iter()
                .find(|category| category.id == discovery.meta.primary)
                .map(|category| category.name.clone())
        } else {
            None
        };
        let at = match which {
            Select::Category => primary_categories(&discovery.categories)
                .iter()
                .position(|category| category.id == discovery.meta.primary),
            Select::Subcategories => all_categories(&discovery.categories)
                .iter()
                .position(|category| discovery.meta.secondary.contains(&category.id)),
            Select::Language => super::audit_spec::LOCALES
                .iter()
                .position(|(locale, _)| *locale == discovery.bits.locale),
        };
        // The list opens with the value in its last visible row (five rows of 40px show at once).
        let scrolled = at.map_or(0., |at| ((at + 1) as f32 * 40. - 200.).max(0.));
        discovery
            .scroll
            .set_offset(gpui::point(px(0.), px(-scrolled)));
        if which != Select::Language
            && let Some(filter) = discovery.filter.clone()
        {
            filter.update(cx, |filter, _| {
                filter.set_text(value.as_deref().unwrap_or_default())
            });
            let handle = gpui::Focusable::focus_handle(filter.read(cx), cx);
            window.focus(&handle, cx);
        }
        cx.notify();
    }

    fn discovery_secondary_at(&self, index: usize) -> Option<u64> {
        let discovery = self.discovery_state()?;
        discovery
            .meta
            .secondary
            .iter()
            .filter(|id| {
                discovery
                    .categories
                    .iter()
                    .any(|category| category.id == **id)
            })
            .nth(index)
            .copied()
    }

    /// `tM.l` in its horizontal layout: the control is as wide as the language it shows.
    fn discovery_language_field(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(discovery) = self.discovery_state() else {
            return div().into_any_element();
        };
        let opened = discovery.open == Some(Select::Language);
        let key = Select::Language.key();
        div()
            .id(key)
            .relative()
            .child(self.anchor_probe(key))
            .flex_none()
            .min_w(px(120.8))
            .h(px(40.))
            .pl(px(12.))
            .pr(px(8.))
            .justify_between()
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
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-strong"))
                    .child(super::server_community::locale_name(&discovery.bits.locale)),
            )
            .child(
                gpui::svg()
                    .path("icons/select_chevron.svg")
                    .size(px(20.))
                    .text_color(color("--text-strong"))
                    .with_transformation(gpui::Transformation::rotate(gpui::percentage(
                        if opened { 0.5 } else { 0. },
                    ))),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.discovery_toggle_select(Select::Language, window, cx)
            }))
            .into_any_element()
    }

    /// The list under an open select.
    pub(super) fn discovery_select_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if !matches!(
            open.page,
            super::server_settings::Page::Access | super::server_settings::Page::Landing
        ) {
            return None;
        }
        let discovery = &open.discovery;
        let which = discovery.open?;
        let field = self
            .tip_anchors
            .borrow()
            .get(which.key())
            .copied()?
            .dilate(px(1.));
        let query = discovery
            .filter
            .as_ref()
            .map(|filter| filter.read(cx).text().to_lowercase())
            .unwrap_or_default();
        let chosen_primary = discovery
            .categories
            .iter()
            .find(|category| category.id == discovery.meta.primary)
            .map(|category| category.name.to_lowercase());
        // The value sits in the box when it opens: until it is edited, nothing is filtered out.
        let query =
            if which == Select::Category && chosen_primary.as_deref() == Some(query.as_str()) {
                String::new()
            } else {
                query
            };
        let (options, current): (Vec<(u64, String)>, Vec<u64>) = match which {
            Select::Category => (
                primary_categories(&discovery.categories)
                    .into_iter()
                    .map(|category| (category.id, category.name.clone()))
                    .collect(),
                vec![discovery.meta.primary],
            ),
            Select::Subcategories => (
                all_categories(&discovery.categories)
                    .into_iter()
                    .map(|category| (category.id, category.name.clone()))
                    .collect(),
                discovery.meta.secondary.clone(),
            ),
            Select::Language => (Vec::new(), Vec::new()),
        };
        let rows: Vec<AnyElement> = if which == Select::Language {
            super::audit_spec::LOCALES
                .iter()
                .enumerate()
                .map(|(index, (value, _))| {
                    let selected = *value == discovery.bits.locale;
                    let value = (*value).to_owned();
                    Self::discovery_option(
                        SharedString::from(format!("discovery-locale-{index}")),
                        super::server_community::locale_name(&value),
                        selected,
                        false,
                        false,
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.discovery_set_locale(&value, cx)),
                    )
                    .into_any_element()
                })
                .collect()
        } else {
            let full = which == Select::Subcategories && current.len() >= 5;
            options
                .into_iter()
                .filter(|(_, name)| query.is_empty() || name.to_lowercase().contains(&query))
                .map(|(id, name)| {
                    let selected = current.contains(&id);
                    let row = Self::discovery_option(
                        SharedString::from(format!("discovery-option-{}-{id}", which.key())),
                        name,
                        selected,
                        full && !selected,
                        which == Select::Subcategories,
                    );
                    if full && !selected {
                        return row.into_any_element();
                    }
                    row.on_click(cx.listener(move |this, _, _, cx| {
                        if which == Select::Category {
                            this.discovery_set_primary(id, cx);
                        } else {
                            this.discovery_toggle_secondary(id, cx);
                        }
                    }))
                    .into_any_element()
                })
                .collect()
        };
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
                    .id("discovery-select-layer")
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
                                && let Some(discovery) = this
                                    .server_settings
                                    .as_mut()
                                    .map(|open| &mut open.discovery)
                            {
                                discovery.open = None;
                                cx.notify();
                            }
                        },
                    ))
                    .child(
                        div()
                            .relative()
                            .child(
                                div()
                                    .id("discovery-select-scroller")
                                    .max_h(px(200.))
                                    .pr(px(16.))
                                    .flex()
                                    .flex_col()
                                    .overflow_y_scroll()
                                    .track_scroll(&discovery.scroll)
                                    .children(rows),
                            )
                            .child(super::settings::auto_scrollbar(discovery.scroll.clone())),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    pub(super) fn discovery_option(
        id: SharedString,
        label: String,
        selected: bool,
        disabled: bool,
        boxed: bool,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .flex_none()
            .h(px(40.))
            .pl(px(8.))
            .pr(px(4.))
            .rounded(px(4.))
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
            })
            .when(disabled, |this| this.opacity(0.5))
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-strong"))
                    .child(label),
            )
            .when(boxed, |this| {
                this.child(super::server_games::checkbox(selected))
            })
            .when(!boxed && selected, |this| {
                this.child(icon("select_check", 20., "--text-strong"))
            })
    }

    /// Module 883494: three promises, then the server is listed.
    pub(super) fn discovery_agreement_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if open.page != super::server_settings::Page::Access {
            return None;
        }
        let boxes = open.discovery.agreement?;
        let rows = [
            (
                "Foster a healthy and positive environment",
                "Create a welcoming space where harassment, toxicity, and illegal activity is not promoted or tolerated.",
            ),
            (
                "Do not host graphic or sexual content",
                "All server content must be appropriate for ages 13+.",
            ),
            (
                "Comply with Discord's Community Guidelines and Discovery Guidelines",
                "Violating our policies may result in penalties against your server, including removal from Discovery.",
            ),
        ];
        let close = |this: &mut Root| {
            if let Some(discovery) = this
                .server_settings
                .as_mut()
                .map(|open| &mut open.discovery)
            {
                discovery.agreement = None;
            }
        };
        let before = "Before you can list your server on Discovery, you must agree to rules to help keep Server Discovery safe for everyone. See Discord's ";
        let (first, second) = ("Community Guidelines", "Discovery Guidelines");
        let text = format!("{before}{first} and {second} for more information.");
        let first_range = before.len()..before.len() + first.len();
        let second_start = before.len() + first.len() + " and ".len();
        let second_range = second_start..second_start + second.len();
        let link = gpui::HighlightStyle {
            color: Some(color("--text-link").into()),
            ..Default::default()
        };
        let styled = gpui::StyledText::new(text)
            .with_highlights([(first_range.clone(), link), (second_range.clone(), link)]);
        let subtitle = gpui::InteractiveText::new("discovery-agreement-links", styled)
            .on_click(vec![first_range, second_range], |index, _, cx| {
                cx.open_url(if index == 0 {
                    "https://support.discord.com/hc/en-us/articles/360035969312"
                } else {
                    "https://support.discord.com/hc/en-us/articles/4409308485271"
                });
            })
            .into_any_element();
        let body = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(rows.into_iter().enumerate().map(|(index, (title, text))| {
                div()
                    .id(SharedString::from(format!("discovery-agreement-{index}")))
                    .flex()
                    .w_full()
                    .gap(px(12.))
                    .px(px(16.))
                    .py(px(12.))
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .cursor_pointer()
                    .child(
                        div()
                            .flex_none()
                            .child(super::server_games::checkbox(boxes[index])),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child(text),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(boxes) = this
                            .server_settings
                            .as_mut()
                            .and_then(|open| open.discovery.agreement.as_mut())
                        {
                            boxes[index] = !boxes[index];
                        }
                        cx.notify();
                    }))
            }))
            .into_any_element();
        let ready = boxes.iter().all(|ticked| *ticked);
        Some(super::modal::render(
            super::modal::Spec {
                id: "discovery-agreement-modal",
                title: "Agree to rules".into(),
                subtitle: Some(subtitle),
                body: Some(body),
                buttons: vec![super::modal::button(
                    "discovery-agreement-list",
                    "List on Discovery",
                    if ready {
                        super::modal::Tone::Primary
                    } else {
                        super::modal::Tone::PrimaryDisabled
                    },
                    move |this, _, cx| {
                        if !ready {
                            return;
                        }
                        let guild = this.server_settings.as_ref().map(|open| open.guild);
                        if let Some(discovery) = this
                            .server_settings
                            .as_mut()
                            .map(|open| &mut open.discovery)
                        {
                            discovery.agreement = None;
                            discovery.agreed = true;
                        }
                        if let Some(guild) = guild {
                            this.save_access(guild, cx);
                        }
                    },
                )],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }
}
