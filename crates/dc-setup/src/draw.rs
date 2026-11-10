//! Painting shared by the setup window and its dialogs: scale, colours, fonts and the few shapes they are made of.
//! One type scale for everything: title 24, body 14, caption 12 (logical pixels, semibold for titles and buttons).
#![allow(unsafe_code)]

use crate::icon_gen;
use dc_tokens::Theme;
use windows::Win32::Foundation::{COLORREF, RECT, SIZE};
use windows::Win32::Graphics::Gdi::*;
use windows::core::w;

/// Sent when the pointer leaves the window after `TrackMouseEvent` asked for it.
pub const WM_MOUSELEAVE: u32 = 0x02A3;

/// Converts logical pixels (laid out at 96 dpi) to device pixels.
#[derive(Clone, Copy)]
pub struct Scale(pub i32);

impl Scale {
    pub fn px(self, value: i32) -> i32 {
        (value * self.0 + 48).div_euclid(96)
    }

    pub fn rect(self, left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left: self.px(left),
            top: self.px(top),
            right: self.px(right),
            bottom: self.px(bottom),
        }
    }
}

pub struct Palette {
    pub window: COLORREF,
    pub panel: COLORREF,
    pub track: COLORREF,
    pub text: COLORREF,
    pub muted: COLORREF,
    pub brand: COLORREF,
    pub brand_hover: COLORREF,
    pub brand_pressed: COLORREF,
    pub secondary: COLORREF,
    pub secondary_hover: COLORREF,
    pub secondary_pressed: COLORREF,
    pub danger: COLORREF,
    pub success: COLORREF,
    pub link: COLORREF,
    pub link_hover: COLORREF,
    pub white: COLORREF,
    pub ring: COLORREF,
}

pub fn token(name: &str) -> (u8, u8, u8) {
    let value = Theme::Dark.color(name);
    ((value >> 24) as u8, (value >> 16) as u8, (value >> 8) as u8)
}

pub fn rgb((r, g, b): (u8, u8, u8)) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn lighten((r, g, b): (u8, u8, u8), amount: u32) -> (u8, u8, u8) {
    let up = |c: u8| (c as u32 + (255 - c as u32) * amount / 100) as u8;
    (up(r), up(g), up(b))
}

fn darken((r, g, b): (u8, u8, u8), amount: u32) -> (u8, u8, u8) {
    let down = |c: u8| (c as u32 * (100 - amount) / 100) as u8;
    (down(r), down(g), down(b))
}

pub fn palette() -> Palette {
    let brand = token("--background-brand");
    let secondary = token("--background-surface-highest");
    let link = token("--text-link");
    Palette {
        window: rgb(token("--background-base-lower")),
        panel: rgb(token("--background-base-lowest")),
        track: rgb(token("--background-surface-highest")),
        text: rgb(token("--text-default")),
        muted: rgb(token("--text-muted")),
        brand: rgb(brand),
        brand_hover: rgb(lighten(brand, 12)),
        brand_pressed: rgb(darken(brand, 16)),
        secondary: rgb(secondary),
        secondary_hover: rgb(lighten(secondary, 10)),
        secondary_pressed: rgb(darken(secondary, 14)),
        danger: rgb(token("--status-danger")),
        success: rgb(token("--status-positive")),
        link: rgb(link),
        link_hover: rgb(lighten(link, 35)),
        white: rgb((255, 255, 255)),
        ring: rgb(token("--text-default")),
    }
}

pub struct Fonts {
    pub title: HFONT,
    pub body: HFONT,
    pub strong: HFONT,
    pub caption: HFONT,
    pub link: HFONT,
}

impl Fonts {
    pub fn new(scale: Scale) -> Self {
        let make = |height: i32, weight: i32, underline: bool| unsafe {
            CreateFontW(
                -scale.px(height),
                0,
                0,
                0,
                weight,
                0,
                underline as u32,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                0,
                w!("Segoe UI"),
            )
        };
        Self {
            title: make(24, 600, false),
            body: make(14, 400, false),
            strong: make(14, 600, false),
            caption: make(12, 400, false),
            link: make(12, 400, true),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            for font in [self.title, self.body, self.strong, self.caption, self.link] {
                let _ = DeleteObject(font.into());
            }
        }
    }
}

/// How a control is drawn right now.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Face {
    pub hover: bool,
    pub pressed: bool,
    pub focus: bool,
}

pub unsafe fn fill(hdc: HDC, rect: RECT, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(hdc, &rect, brush);
        let _ = DeleteObject(brush.into());
    }
}

/// A filled rounded rectangle with the given corner radius (GDI wants the corner diameter, so it is doubled here).
pub unsafe fn round(hdc: HDC, rect: RECT, radius: i32, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(hdc, brush.into());
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        let _ = RoundRect(
            hdc,
            rect.left,
            rect.top,
            rect.right + 1,
            rect.bottom + 1,
            radius * 2,
            radius * 2,
        );
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
        let _ = DeleteObject(brush.into());
    }
}

/// A filled circle that fills `rect`.
pub unsafe fn disc(hdc: HDC, rect: RECT, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(hdc, brush.into());
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        let _ = Ellipse(hdc, rect.left, rect.top, rect.right + 1, rect.bottom + 1);
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
        let _ = DeleteObject(brush.into());
    }
}

/// The keyboard focus ring: a two pixel outline just outside `rect`.
pub unsafe fn ring(hdc: HDC, rect: RECT, radius: i32, colors: &Palette, scale: Scale) {
    unsafe {
        let width = scale.px(2).max(2);
        let gap = scale.px(3);
        let pen = CreatePen(PS_SOLID, width, colors.ring);
        let old_pen = SelectObject(hdc, pen.into());
        let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
        let diameter = (radius + gap) * 2;
        let _ = RoundRect(
            hdc,
            rect.left - gap,
            rect.top - gap,
            rect.right + gap,
            rect.bottom + gap,
            diameter,
            diameter,
        );
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        let _ = DeleteObject(pen.into());
    }
}

/// A check mark of the given colour inside `rect`.
pub unsafe fn tick(hdc: HDC, rect: RECT, color: COLORREF, scale: Scale) {
    unsafe {
        let pen = CreatePen(PS_SOLID, scale.px(2).max(2), color);
        let old = SelectObject(hdc, pen.into());
        let size = rect.right - rect.left;
        let (x, y) = (rect.left, rect.top);
        let _ = MoveToEx(hdc, x + size * 24 / 100, y + size * 52 / 100, None);
        let _ = LineTo(hdc, x + size * 43 / 100, y + size * 70 / 100);
        let _ = LineTo(hdc, x + size * 77 / 100, y + size * 32 / 100);
        SelectObject(hdc, old);
        let _ = DeleteObject(pen.into());
    }
}

pub unsafe fn text(
    hdc: HDC,
    font: HFONT,
    color: COLORREF,
    mut rect: RECT,
    value: &str,
    format: DRAW_TEXT_FORMAT,
) {
    let mut wide: Vec<u16> = value.encode_utf16().collect();
    unsafe {
        SelectObject(hdc, font.into());
        SetTextColor(hdc, color);
        DrawTextW(hdc, &mut wide, &mut rect, format);
    }
}

/// The width of `value` on one line in `font`.
pub unsafe fn text_width(hdc: HDC, font: HFONT, value: &str) -> i32 {
    let wide: Vec<u16> = value.encode_utf16().collect();
    let mut size = SIZE::default();
    unsafe {
        SelectObject(hdc, font.into());
        let _ = GetTextExtentPoint32W(hdc, &wide, &mut size);
    }
    size.cx
}

/// The height `value` needs when wrapped to `width`.
pub unsafe fn wrapped_height(hdc: HDC, font: HFONT, width: i32, value: &str) -> i32 {
    let mut wide: Vec<u16> = value.encode_utf16().collect();
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: 0,
    };
    unsafe {
        SelectObject(hdc, font.into());
        DrawTextW(
            hdc,
            &mut wide,
            &mut rect,
            DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
        );
    }
    rect.bottom
}

/// Wrapped text centred vertically in `rect`; whatever does not fit ends in an ellipsis.
pub unsafe fn wrapped_centered(hdc: HDC, font: HFONT, color: COLORREF, rect: RECT, value: &str) {
    unsafe {
        let height = wrapped_height(hdc, font, rect.right - rect.left, value);
        let top = rect.top + ((rect.bottom - rect.top - height) / 2).max(0);
        text(
            hdc,
            font,
            color,
            RECT { top, ..rect },
            value,
            DT_LEFT | DT_WORDBREAK | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
    }
}

/// A push button: brand colour when primary, a raised surface otherwise, with hover, pressed and focus looks.
pub struct ButtonView<'a> {
    pub rect: RECT,
    pub label: &'a str,
    pub primary: bool,
    pub face: Face,
}

pub unsafe fn button(hdc: HDC, fonts: &Fonts, colors: &Palette, scale: Scale, view: &ButtonView) {
    let face = view.face;
    let pressed = face.pressed && face.hover;
    let fill_color = match (view.primary, pressed, face.hover) {
        (true, true, _) => colors.brand_pressed,
        (true, false, true) => colors.brand_hover,
        (true, false, false) => colors.brand,
        (false, true, _) => colors.secondary_pressed,
        (false, false, true) => colors.secondary_hover,
        (false, false, false) => colors.secondary,
    };
    unsafe {
        round(hdc, view.rect, scale.px(8), fill_color);
        text(
            hdc,
            fonts.strong,
            colors.white,
            view.rect,
            view.label,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
        );
        if face.focus {
            ring(hdc, view.rect, scale.px(8), colors, scale);
        }
    }
}

/// A checkbox row: the box at the left of `rect`, the label after it.
pub struct CheckView<'a> {
    pub rect: RECT,
    pub label: &'a str,
    pub on: bool,
    pub face: Face,
}

pub unsafe fn checkbox(hdc: HDC, fonts: &Fonts, colors: &Palette, scale: Scale, view: &CheckView) {
    let CheckView {
        rect,
        label,
        on,
        face,
    } = *view;
    let size = scale.px(20);
    let top = rect.top + (rect.bottom - rect.top - size) / 2;
    let boxed = RECT {
        left: rect.left,
        top,
        right: rect.left + size,
        bottom: top + size,
    };
    let pressed = face.pressed && face.hover;
    unsafe {
        if on {
            let color = if pressed {
                colors.brand_pressed
            } else if face.hover {
                colors.brand_hover
            } else {
                colors.brand
            };
            round(hdc, boxed, scale.px(5), color);
            tick(hdc, boxed, colors.white, scale);
        } else {
            let edge = if face.hover {
                colors.text
            } else {
                colors.muted
            };
            round(hdc, boxed, scale.px(5), edge);
            let line = scale.px(2).max(2);
            let inner = RECT {
                left: boxed.left + line,
                top: boxed.top + line,
                right: boxed.right - line,
                bottom: boxed.bottom - line,
            };
            let center = if pressed { colors.track } else { colors.window };
            round(hdc, inner, (scale.px(5) - line).max(2), center);
        }
        text(
            hdc,
            fonts.body,
            colors.text,
            RECT {
                left: boxed.right + scale.px(10),
                ..rect
            },
            label,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
        );
        if face.focus {
            ring(hdc, boxed, scale.px(5), colors, scale);
        }
    }
}

/// The application mark at (`x`, `y`), composited on the window colour so it needs no alpha blit.
pub unsafe fn logo(hdc: HDC, colors: &Palette, x: i32, y: i32, size: i32) {
    let pixels = icon_gen::render_rgba(size as u32);
    let background = [
        colors.window.0 & 0xff,
        (colors.window.0 >> 8) & 0xff,
        (colors.window.0 >> 16) & 0xff,
    ];
    let mut bgra = Vec::with_capacity(pixels.len());
    for pixel in pixels.chunks(4) {
        let alpha = pixel[3] as u32;
        let mixed = |channel: usize, base: u32| {
            ((pixel[channel] as u32 * alpha + base * (255 - alpha)) / 255) as u8
        };
        bgra.extend([
            mixed(2, background[2]),
            mixed(1, background[1]),
            mixed(0, background[0]),
            255,
        ]);
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        SetDIBitsToDevice(
            hdc,
            x,
            y,
            size as u32,
            size as u32,
            0,
            0,
            0,
            size as u32,
            bgra.as_ptr().cast(),
            &info,
            DIB_RGB_COLORS,
        );
    }
}
