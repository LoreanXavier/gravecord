//! Tooltips. Box from `tooltip_ZDdyhE`, caret from module 160733, trigger from 402883, spring from 280336, guild body from 396655.

use super::{Root, SANS, badge_paths as paths, color, kerning};
use dc_state::{Badge, Guild, Id};
use gpui::{
    Animation, AnimationExt, AnyElement, App, Bounds, BoxShadow, Context, Display, Element,
    ElementId, FontWeight, GlobalElementId, Image, ImageFormat, InspectorElementId, IntoElement,
    LayoutId, Pixels, Position, Radians, Rgba, SharedString, Stateful, Style, Transformation,
    Window, canvas, deferred, div, hsla, img, point, prelude::*, px, svg,
};
use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::Arc;
use std::time::Duration;

/// `spacing` default in module 160733.
pub(super) const SPACING: f32 = 11.;
/// `viewportPadding` default in module 508382.
const VIEWPORT_PADDING: f32 = 8.;
const MAX_WIDTH: f32 = 200.;
/// Past this the spring from module 280336 has settled.
const FADE: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Side {
    Top,
    Bottom,
    /// In the official set; nothing built so far opens to the left.
    #[allow(dead_code)]
    Left,
    Right,
}

#[derive(Clone)]
pub(super) enum Body {
    Text(SharedString),
    /// The rail's text, in the guild tooltip's `body-md/semibold`.
    Heading(SharedString),
    Guild(Id),
    /// `richTooltipContent`: a medium title over muted text, both `body-sm`.
    Rich(SharedString, SharedString),
}

#[derive(Clone)]
pub(super) struct Tip {
    pub body: Body,
    pub side: Side,
    pub spacing: f32,
}

impl Tip {
    pub fn text(text: &'static str, side: Side) -> Self {
        Self {
            body: Body::Text(text.into()),
            side,
            spacing: SPACING,
        }
    }

    pub fn body(body: Body, side: Side) -> Self {
        Self {
            body,
            side,
            spacing: SPACING,
        }
    }

    /// The server rail keeps its tooltips 12px from the icon.
    pub fn rail(body: Body) -> Self {
        Self {
            body,
            side: Side::Right,
            spacing: 12.,
        }
    }
}

pub(super) struct Active {
    key: SharedString,
    tip: Tip,
    anchor: Bounds<Pixels>,
}

impl Active {
    /// A tooltip placed by its owner rather than by hover, like a slider's value bubble.
    pub fn new(key: SharedString, tip: Tip, anchor: Bounds<Pixels>) -> Self {
        Self { key, tip, anchor }
    }

    pub fn key(&self) -> &str {
        &self.key
    }
}

/// `{tension:2400,friction:52}` with react-spring's default mass, clamped because opacity cannot pass 1.
fn spring(delta: f32) -> f32 {
    if delta >= 1. {
        return 1.;
    }
    let time = delta * FADE.as_secs_f32();
    let natural = 2400f32.sqrt();
    let decay = 52. / 2.;
    let damped = (natural * natural - decay * decay).sqrt();
    let envelope = (-decay * time).exp();
    let value = 1. - envelope * ((damped * time).cos() + decay / damped * (damped * time).sin());
    value.clamp(0., 1.)
}

/// Where the box goes around its target, pushed along the target's edge to stay in the viewport.
fn place(
    anchor: Bounds<Pixels>,
    body: gpui::Size<Pixels>,
    side: Side,
    spacing: Pixels,
    viewport: gpui::Size<Pixels>,
) -> gpui::Point<Pixels> {
    let center = anchor.center();
    let origin = match side {
        Side::Top => point(
            center.x - body.width / 2.,
            anchor.top() - spacing - body.height,
        ),
        Side::Bottom => point(center.x - body.width / 2., anchor.bottom() + spacing),
        Side::Left => point(
            anchor.left() - spacing - body.width,
            center.y - body.height / 2.,
        ),
        Side::Right => point(anchor.right() + spacing, center.y - body.height / 2.),
    };
    let padding = px(VIEWPORT_PADDING);
    let clamp = |value: Pixels, length: Pixels, limit: Pixels| {
        value.min(limit - padding - length).max(padding)
    };
    match side {
        Side::Top | Side::Bottom => point(clamp(origin.x, body.width, viewport.width), origin.y),
        Side::Left | Side::Right => point(origin.x, clamp(origin.y, body.height, viewport.height)),
    }
}

/// Top left of the 16x10 caret, before rotation. Its 16px box sits 5px over the edge and it rotates about that box's center.
fn caret_origin(anchor: Bounds<Pixels>, body: Bounds<Pixels>, side: Side) -> gpui::Point<Pixels> {
    let target = anchor.center();
    // Limits from `A` in module 160733.
    let reach_x = body.size.width / 2. - px(8.);
    let reach_y = body.size.height / 2. - px(8.);
    let center = body.center();
    let along_x = (target.x - center.x).max(-reach_x).min(reach_x);
    let along_y = (target.y - center.y).max(-reach_y).min(reach_y);
    let box_center = match side {
        // The box is above its target, so the caret hangs under the box.
        Side::Top => point(center.x + along_x, body.bottom() + px(3.)),
        Side::Bottom => point(center.x + along_x, body.top() - px(3.)),
        Side::Left => point(body.right() + px(3.), center.y + along_y + px(1.)),
        Side::Right => point(body.left() - px(3.), center.y + along_y + px(1.)),
    };
    point(box_center.x - px(8.), box_center.y - px(5.))
}

struct Placed {
    anchor: Bounds<Pixels>,
    side: Side,
    spacing: Pixels,
    body: AnyElement,
    caret: AnyElement,
}

impl IntoElement for Placed {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Placed {
    type RequestLayoutState = (LayoutId, LayoutId);
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let body = self.body.request_layout(window, cx);
        let caret = self.caret.request_layout(window, cx);
        let style = Style {
            position: Position::Absolute,
            display: Display::Flex,
            ..Style::default()
        };
        let layout = window.request_layout(style, [body, caret], cx);
        (layout, (body, caret))
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (body, caret): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let laid_body = window.layout_bounds(*body);
        let laid_caret = window.layout_bounds(*caret);
        let origin = place(
            self.anchor,
            laid_body.size,
            self.side,
            self.spacing,
            window.viewport_size(),
        );
        let placed = Bounds::new(origin, laid_body.size);
        let caret_at = caret_origin(self.anchor, placed, self.side);
        window.with_element_offset(origin - laid_body.origin, |window| {
            self.body.prepaint(window, cx);
        });
        window.with_element_offset(caret_at - laid_caret.origin, |window| {
            self.caret.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.body.paint(window, cx);
        self.caret.paint(window, cx);
    }
}

fn caret(side: Side) -> impl IntoElement {
    // The drawing points down; turn it to point at the target.
    let turn = match side {
        Side::Top => 0.,
        Side::Bottom => PI,
        Side::Left => -FRAC_PI_2,
        Side::Right => FRAC_PI_2,
    };
    let layer = |path: &'static str, tint: &str| {
        svg()
            .absolute()
            .w(px(16.))
            .h(px(10.))
            .path(path)
            .text_color(color(tint))
            .with_transformation(Transformation::rotate(Radians(turn)))
    };
    div()
        .relative()
        .flex_none()
        .w(px(16.))
        .h(px(10.))
        .child(layer(
            "icons/tooltip_caret_fill.svg",
            "--background-surface-high",
        ))
        .child(layer("icons/tooltip_caret_stroke.svg", "--border-subtle"))
}

fn frame() -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .max_w(px(MAX_WIDTH))
        // A 1px inset shadow in the original, so the border comes out of the padding.
        .px(px(11.))
        .py(px(7.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-surface-high"))
        .shadow(vec![BoxShadow {
            color: hsla(0., 0., 0., 0.24),
            offset: point(px(0.), px(12.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        }])
        .font_family(SANS)
        .font_features(kerning())
        .text_color(color("--text-default"))
}

fn hex(token: &str) -> String {
    let rgba: Rgba = color(token);
    let byte = |value: f32| (value * 255.).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        byte(rgba.r),
        byte(rgba.g),
        byte(rgba.b)
    )
}

fn paths_svg(paths: &[&str], fill: &str) -> String {
    paths
        .iter()
        .map(|path| format!(r#"<path fill="{fill}" fill-rule="evenodd" d="{path}"/>"#))
        .collect()
}

/// The table `V` in module 554783, dark theme: icon, foreground, background, size adjustment.
fn badge_svg(badge: Badge, premium: bool, side: u32) -> Option<String> {
    let open = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{side}" height="{side}" viewBox="0 0 {side} {side}">"#
    );
    if badge == Badge::Discoverable {
        let scale = side as f32 / 24.;
        return Some(format!(
            r#"{open}<g transform="scale({scale})"><path fill="{}" d="{}"/>{}</g></svg>"#,
            hex("--badge-inverted-background"),
            paths::WEB_BACKGROUND,
            paths_svg(paths::WEB_FOREGROUND, &hex("--badge-inverted-text")),
        ));
    }
    let white = "#ffffff".to_owned();
    let ((view_box, icon), foreground, background, adjust) = match badge {
        Badge::Staff => (paths::STAFF, white, hex("--green-360"), 0),
        Badge::Verified | Badge::VerifiedAndPartnered => {
            (paths::CHECK, white, hex("--green-360"), 0)
        }
        Badge::Partnered => (paths::PARTNER, white, hex("--brand-500"), 0),
        Badge::Community if premium => (paths::HOME, white, "url(#premium)".to_owned(), 2),
        Badge::Community => (paths::HOME, hex("--primary-500"), white, 2),
        Badge::Discoverable | Badge::None => return None,
    };
    let inner = (side as f32 * 0.75).floor() as u32 - adjust;
    let inset = (side as f32 - inner as f32) / 2.;
    let scale = side as f32 / 16.;
    Some(format!(
        r#"{open}<defs><linearGradient id="premium" gradientTransform="rotate(45)"><stop offset="0" stop-color="{}"/><stop offset="1" stop-color="{}"/></linearGradient></defs><g transform="scale({scale})"><path fill="{background}" fill-rule="evenodd" d="{}"/></g><svg x="{inset}" y="{inset}" width="{inner}" height="{inner}" viewBox="{view_box}">{}</svg></svg>"#,
        hex("--premium-tier-2-purple"),
        hex("--premium-tier-2-pink"),
        paths::FLOWER,
        paths_svg(icon, &foreground),
    ))
}

fn badge_image(guild: &Guild) -> Option<Arc<Image>> {
    let svg = badge_svg(guild.badge(), guild.premium(), 18)?;
    Some(Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        svg.into_bytes(),
    )))
}

impl Root {
    /// Records the bounds of its parent (it fills it) under `key` in `tip_anchors`, for popouts that open beside it.
    pub(super) fn anchor_probe(&self, key: impl Into<SharedString>) -> impl IntoElement {
        let anchors = self.tip_anchors.clone();
        let key: SharedString = key.into();
        canvas(
            move |bounds, _, _| {
                anchors.borrow_mut().insert(key.clone(), bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// Puts a tooltip on `target`: it opens on hover and closes on leave or any press, as module 402883 does.
    pub(super) fn with_tip(
        &self,
        key: impl Into<SharedString>,
        tip: Tip,
        target: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let key: SharedString = key.into();
        let anchors = self.tip_anchors.clone();
        let (stored, hovered) = (key.clone(), key.clone());
        div()
            .id(ElementId::Name(format!("tip-{key}").into()))
            .relative()
            .flex_none()
            .child(target)
            .child(
                canvas(
                    move |bounds, _, _| {
                        anchors.borrow_mut().insert(stored.clone(), bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                if *inside {
                    let anchor = this.tip_anchors.borrow().get(&hovered).copied();
                    this.tip = anchor.map(|anchor| Active {
                        key: hovered.clone(),
                        tip: tip.clone(),
                        anchor,
                    });
                } else if this.tip.as_ref().is_some_and(|tip| tip.key == hovered) {
                    this.tip = None;
                }
                cx.notify();
            }))
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                this.tip = None;
                cx.notify();
            }))
    }

    fn tip_body(&self, body: &Body) -> Option<AnyElement> {
        Some(match body {
            Body::Text(text) => frame()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_center()
                .child(div().max_w(px(MAX_WIDTH - 24.)).child(text.clone()))
                .into_any_element(),
            Body::Heading(text) => frame()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(600.))
                .child(div().max_w(px(MAX_WIDTH - 24.)).child(text.clone()))
                .into_any_element(),
            Body::Rich(title, body) => frame()
                .text_size(px(14.))
                .line_height(px(18.))
                .child(
                    div()
                        .max_w(px(MAX_WIDTH - 24.))
                        .font_weight(FontWeight(500.))
                        .child(title.clone()),
                )
                .child(
                    div()
                        .max_w(px(MAX_WIDTH - 24.))
                        .text_color(color("--text-subtle"))
                        .child(body.clone()),
                )
                .into_any_element(),
            Body::Guild(id) => {
                let guild = self.stores.guild(*id)?;
                frame()
                    .child(
                        div()
                            .max_w(px(MAX_WIDTH - 24.))
                            .flex()
                            .items_start()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .children(badge_image(guild).map(|badge| {
                                img(badge).flex_none().mt(px(2.)).mr(px(8.)).size(px(18.))
                            }))
                            .child(div().min_w(px(0.)).line_clamp(2).child(guild.name.clone())),
                    )
                    .into_any_element()
            }
        })
    }

    pub(super) fn tip_layer(&self) -> Option<AnyElement> {
        let active = self.tip.as_ref()?;
        let body = self.tip_body(&active.tip.body)?;
        let key = active.key.clone();
        // Reduced motion shows a tip at once instead of fading it in.
        let fade = if self.pref_on("reduced_motion", false) {
            Duration::from_millis(1)
        } else {
            FADE
        };
        let body = div()
            .child(body)
            .with_animation(
                ElementId::Name(format!("tip-body-{key}").into()),
                Animation::new(fade).with_easing(spring),
                |element, delta| element.opacity(delta),
            )
            .into_any_element();
        let caret = div()
            .child(caret(active.tip.side))
            .with_animation(
                ElementId::Name(format!("tip-caret-{key}").into()),
                Animation::new(fade).with_easing(spring),
                |element, delta| element.opacity(delta),
            )
            .into_any_element();
        Some(
            deferred(Placed {
                anchor: active.anchor,
                side: active.tip.side,
                spacing: px(active.tip.spacing),
                body,
                caret,
            })
            .with_priority(2)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Side, caret_origin, place, spring};
    use gpui::{Bounds, point, px, size};

    fn anchor() -> Bounds<gpui::Pixels> {
        Bounds::new(point(px(16.), px(137.)), size(px(40.), px(40.)))
    }

    #[test]
    fn the_guild_tooltip_lands_where_the_capture_has_it() {
        let viewport = size(px(1280.), px(720.));
        let body = size(px(200.), px(56.));
        let origin = place(anchor(), body, Side::Right, px(12.), viewport);
        assert_eq!(origin, point(px(68.), px(129.)));
        let caret = caret_origin(anchor(), Bounds::new(origin, body), Side::Right);
        // Unrotated box of the capture's caret, which turns into 60..70 by 150..166.
        assert_eq!(caret, point(px(57.), px(153.)));
    }

    #[test]
    fn a_tooltip_at_the_window_edge_slides_in() {
        let viewport = size(px(300.), px(720.));
        let target = Bounds::new(point(px(280.), px(10.)), size(px(20.), px(20.)));
        let origin = place(
            target,
            size(px(100.), px(34.)),
            Side::Bottom,
            px(11.),
            viewport,
        );
        assert_eq!(origin.x, px(192.));
    }

    #[test]
    fn the_spring_starts_at_zero_and_ends_at_one() {
        assert_eq!(spring(0.), 0.);
        assert_eq!(spring(1.), 1.);
        assert!(spring(0.2) > 0.5);
    }
}
