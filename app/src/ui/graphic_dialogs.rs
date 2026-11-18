//! Dialogs on the design system's graphic modal (module 772707: `size md`, a blue gradient header with the graphic,
//! the title and the subtitle, then the action bar with full-width buttons), laid out as the Dangerous Website Ahead
//! dialog measured it:
//!
//! * Potential Dangerous Download (module 574563, shown by 788995): a link whose path ends in a file kind that can harm
//!   the device (`m` of module 432518) asks before it opens, after the blocked-domain check of `Y` in module 889558.
//!   Close stays; Continue to download opens the link.
//! * Urgent Message (module 752313, shown by 578818): at connection open and on a new message, an account with
//!   `HAS_UNREAD_URGENT_MESSAGES` that is not looking at the DM with the Discord system account sees it once; Okay
//!   opens that DM. Opening the DM clears the flag (`setFlag`, `PATCH /users/@me`).

use super::channel_status::{HEADER_WIDTH, MODAL_WIDTH, header_gradient};
use super::{Root, color, icon};
use crate::parity::Parity;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, deferred, div, img, prelude::*, px, rgba,
};

/// The graphic of 574563.
const DOWNLOAD_GRAPHIC: &str = "https://cdn.discordapp.com/assets/content/40db7db5eefbabf36afe02e04e420d2306c57a91a34bebb7da82ca6bee9a8b2a.svg";
/// Module 31008, the graphic of 752313.
const URGENT_GRAPHIC: &str = "https://cdn.discordapp.com/assets/content/88cde2b0ab4c8015cee8fdb8732b85b01df4fc78a75b3aa5e621539ea94b1803.svg";
/// `K` of module 716371: the Discord system account whose DM holds urgent messages.
const SYSTEM_USER: dc_state::Id = 643_945_264_868_098_049;
/// `UserFlags.HAS_UNREAD_URGENT_MESSAGES` (module 652215).
const HAS_UNREAD_URGENT_MESSAGES: u64 = 8192;

/// The file kinds of module 432518.
const DANGEROUS: [&str; 149] = [
    "7z",
    "ade",
    "adp",
    "arj",
    "apk",
    "appimage",
    "application",
    "appx",
    "appxbundle",
    "asx",
    "bas",
    "bat",
    "cab",
    "cer",
    "chm",
    "cmd",
    "cnt",
    "cpl",
    "crt",
    "csh",
    "deb",
    "der",
    "diagcab",
    "dll",
    "dmg",
    "docm",
    "dotm",
    "ex",
    "ex_",
    "exe",
    "flatpak",
    "flatpakref",
    "fxp",
    "gadget",
    "grp",
    "gz",
    "hlp",
    "hpj",
    "hta",
    "htc",
    "inf",
    "ins",
    "ipa",
    "iso",
    "isp",
    "its",
    "jar",
    "jnlp",
    "jse",
    "ksh",
    "lib",
    "lnk",
    "mad",
    "maf",
    "mag",
    "mam",
    "maq",
    "mar",
    "mas",
    "mat",
    "mau",
    "mav",
    "maw",
    "mcf",
    "mda",
    "mdb",
    "mde",
    "mdt",
    "mdw",
    "mdz",
    "msc",
    "msh",
    "msh1",
    "msh1xml",
    "msh2",
    "msh2xml",
    "mshxml",
    "msi",
    "msix",
    "msixbundle",
    "msp",
    "mst",
    "msu",
    "nsh",
    "ops",
    "osd",
    "pcd",
    "pif",
    "pkg",
    "pl",
    "plg",
    "potm",
    "ppam",
    "ppsm",
    "pptm",
    "prf",
    "prg",
    "printerexport",
    "ps1",
    "ps1xml",
    "ps2",
    "ps2xml",
    "psc1",
    "psc2",
    "psd1",
    "psdm1",
    "pst",
    "py",
    "pyc",
    "pyo",
    "pyw",
    "pyz",
    "pyzw",
    "rar",
    "reg",
    "rpm",
    "scf",
    "scr",
    "sct",
    "shb",
    "shs",
    "sldm",
    "snap",
    "sys",
    "theme",
    "tmp",
    "url",
    "vb",
    "vbe",
    "vbp",
    "vbs",
    "vhd",
    "vhdx",
    "vsmacros",
    "vsw",
    "vxd",
    "webpnp",
    "ws",
    "wsc",
    "wsf",
    "wsh",
    "xbap",
    "xlam",
    "xll",
    "xlsm",
    "xltm",
    "xnk",
    "z",
    "zip",
];

/// One open graphic dialog.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum GraphicDialog {
    DangerousDownload { url: String },
    UrgentMessage,
}

/// The decoded path of a URL (`toURLSafe(...).pathname`), with its host; `None` for something that is not a URL.
fn host_and_path(url: &str) -> Option<(String, String)> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority
        .rsplit('@')
        .next()?
        .split(':')
        .next()?
        .to_ascii_lowercase();
    let after = &rest[end..];
    let path = after[..after.find(['?', '#']).unwrap_or(after.len())].to_owned();
    Some((
        host,
        if path.is_empty() {
            "/".to_owned()
        } else {
            path
        },
    ))
}

/// `decodeURIComponent`, keeping the text when it does not decode.
fn decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            match text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                Some(byte) => {
                    out.push(byte);
                    index += 3;
                    continue;
                }
                None => return text.to_owned(),
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// `m` of module 432518: the dangerous file kind a link's path ends in. On github.com, bitbucket.org and gitlab.com
/// only download paths count. The last segment (after `.` and `..` are walked) needs a dot; its kind is lower-cased.
pub(super) fn dangerous_extension(url: &str) -> Option<String> {
    let (host, path) = host_and_path(url)?;
    let download = match host.as_str() {
        "github.com" => {
            path.contains("/releases") && path.contains("/download")
                || path.contains("archive/refs/")
                || path.contains("/i/raw/i/")
                || path.contains("/user-attachments")
        }
        "bitbucket.org" | "gitlab.com" => path.contains("/downloads"),
        _ => true,
    };
    if !download {
        return None;
    }
    let path = decoded(&path);
    let segments: Vec<&str> = path.split('/').collect();
    let mut up = 0usize;
    let mut last = None;
    for index in (0..segments.len()).rev() {
        let segment = segments[index];
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            up += 1;
            continue;
        }
        if up > index {
            break;
        }
        last = Some(segments[index - up]);
        break;
    }
    let mut parts: Vec<&str> = last?.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let kind = parts.pop()?.to_ascii_lowercase();
    DANGEROUS.contains(&kind.as_str()).then_some(kind)
}

impl Root {
    /// `E` of module 578818 on `POST_CONNECTION_OPEN` (READY here) and `MESSAGE_CREATE`; `A` when the system DM is
    /// open: the flag is cleared and the dialog may show again later.
    pub(super) fn urgent_message_event(&mut self, name: &str) {
        if !matches!(name, "READY" | "MESSAGE_CREATE") {
            return;
        }
        let Some(me) = self.stores.current_user.as_ref() else {
            return;
        };
        if (me.flags | me.public_flags) & HAS_UNREAD_URGENT_MESSAGES == 0 {
            return;
        }
        let system_dm = self.system_dm();
        if system_dm.is_some() && system_dm == self.channel && self.guild.is_none() {
            self.urgent_message_seen();
            return;
        }
        if !self.chat_extra.urgent_shown {
            self.chat_extra.urgent_shown = true;
            self.chat_extra.graphic = Some(GraphicDialog::UrgentMessage);
        }
    }

    /// `getDMFromUserId(K)`.
    fn system_dm(&self) -> Option<dc_state::Id> {
        self.stores
            .private_channels
            .iter()
            .find(|channel| channel.kind == 1 && channel.recipients.contains(&SYSTEM_USER))
            .map(|channel| channel.id)
    }

    /// `A` of module 578818 (`CHANNEL_SELECT`): opening the system DM clears the flag.
    pub(super) fn urgent_message_channel_selected(&mut self, channel: Option<dc_state::Id>) {
        let Some(me) = self.stores.current_user.as_ref() else {
            return;
        };
        if me.flags & HAS_UNREAD_URGENT_MESSAGES != 0
            && channel.is_some()
            && channel == self.system_dm()
        {
            self.urgent_message_seen();
        }
    }

    /// `setFlag(HAS_UNREAD_URGENT_MESSAGES, false)`.
    fn urgent_message_seen(&mut self) {
        self.chat_extra.urgent_shown = false;
        let Some(me) = self.stores.current_user.as_ref() else {
            return;
        };
        let flags = me.flags & !HAS_UNREAD_URGENT_MESSAGES;
        let _ = self.requests.send(crate::net::Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: "/users/@me".to_owned(),
            body: Some(serde_json::json!({ "flags": flags })),
        });
    }

    /// Capture only: `graphic urgent` turns the flag on locally and runs the check as a new message would;
    /// `graphic download <url>` follows a link as a click does.
    #[cfg(feature = "capture")]
    pub(crate) fn script_graphic(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        match words {
            ["urgent"] => {
                if let Some(me) = self.stores.current_user.as_mut() {
                    me.flags |= HAS_UNREAD_URGENT_MESSAGES;
                }
                self.urgent_message_event("MESSAGE_CREATE");
                cx.notify();
                format!("graphic: {:?}", self.chat_extra.graphic)
            }
            ["download", url] => {
                self.follow_link(url, "", cx);
                format!("graphic: {:?}", self.chat_extra.graphic)
            }
            _ => "usage: graphic urgent | graphic download <url>".to_owned(),
        }
    }

    fn graphic_action(&mut self, primary: bool, cx: &mut Context<Self>) {
        let Some(dialog) = self.chat_extra.graphic.take() else {
            return;
        };
        match dialog {
            GraphicDialog::DangerousDownload { url } => {
                if !primary {
                    cx.open_url(&url);
                }
            }
            GraphicDialog::UrgentMessage => {
                if let Some(channel) = self.system_dm() {
                    self.select_home(Some(channel));
                }
            }
        }
        cx.notify();
    }

    /// Escape closes the open graphic dialog; true when there was one.
    pub(super) fn graphic_dialog_escape(&mut self) -> bool {
        self.chat_extra.graphic.take().is_some()
    }

    pub(super) fn graphic_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.chat_extra.graphic.as_ref()?;
        let (graphic, title, subtitle, actions): (_, _, _, Vec<(&str, bool)>) = match dialog {
            GraphicDialog::DangerousDownload { .. } => (
                DOWNLOAD_GRAPHIC,
                dc_intl::t("XtDo9Z"),
                dc_intl::t("L9yFko"),
                vec![(dc_intl::t("j7Vi2i"), true), (dc_intl::t("/bHu89"), false)],
            ),
            GraphicDialog::UrgentMessage => (
                URGENT_GRAPHIC,
                dc_intl::t("bAhz9l"),
                dc_intl::t("7KjxW3"),
                vec![(dc_intl::t("BddRzS"), true)],
            ),
        };
        let art = self.images.get(graphic).cloned();
        if art.is_none() {
            self.want_image(graphic.to_owned(), graphic.to_owned(), cx);
        }
        let gradient = header_gradient(
            color("--expressive-gradient-blue-start"),
            color("--expressive-gradient-blue-end"),
        );
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
                    .child(title.to_owned()),
            )
            .child(
                div()
                    .mt(px(8.))
                    .w_full()
                    .text_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-subtle"))
                    .child(subtitle.to_owned()),
            );
        let footer = div()
            .flex_none()
            .px(px(32.))
            .py(px(16.))
            .flex()
            .gap(px(8.))
            .children(
                actions
                    .into_iter()
                    .enumerate()
                    .map(|(index, (label, primary))| {
                        let (border, background, hover, text) = if primary {
                            (
                                "--control-primary-border-default",
                                "--control-primary-background-default",
                                "--control-primary-background-hover",
                                "--white",
                            )
                        } else {
                            (
                                "--border-muted",
                                "--control-secondary-background-default",
                                "--control-secondary-background-hover",
                                "--control-secondary-text-default",
                            )
                        };
                        div()
                            .id(("graphic-dialog-action", index))
                            .flex_1()
                            .h(px(40.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color(border))
                            .bg(color(background))
                            .hover(|style| style.bg(color(hover)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color(text))
                            .child(label.to_owned())
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.graphic_action(primary, cx)),
                            )
                    }),
            );
        let pose = crate::ui::motion::modal_pose("graphic-dialog");
        let panel = div()
            .parity_with_border("graphic-dialog", 1.)
            .id("graphic-dialog")
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
            // No body (`r.c` without children): the action bar follows the header.
            .child(header)
            .child(footer)
            .child(
                div()
                    .id("graphic-dialog-close")
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
                        this.chat_extra.graphic = None;
                        cx.notify();
                    })),
            );
        Some(
            deferred(
                div()
                    .id("graphic-dialog-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.chat_extra.graphic = None;
                        cx.notify();
                    }))
                    .child(panel),
            )
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::dangerous_extension;

    #[test]
    fn dangerous_kinds_count_at_the_end_of_the_path_only() {
        assert_eq!(
            dangerous_extension("https://example.com/setup.EXE").as_deref(),
            Some("exe")
        );
        assert_eq!(
            dangerous_extension("https://example.com/a/b.zip?x=1#y").as_deref(),
            Some("zip")
        );
        assert_eq!(
            dangerous_extension("https://example.com/my%20tool.msi").as_deref(),
            Some("msi")
        );
        assert_eq!(
            dangerous_extension("https://example.com/tool.exe/").as_deref(),
            Some("exe")
        );
        assert_eq!(
            dangerous_extension("https://example.com/a.exe/b/..").as_deref(),
            Some("exe")
        );
        assert_eq!(dangerous_extension("https://example.com/readme.txt"), None);
        assert_eq!(dangerous_extension("https://example.com/exe"), None);
        assert_eq!(dangerous_extension("https://example.com/"), None);
        assert_eq!(dangerous_extension("not a url"), None);
        // On code hosts only download paths count.
        assert_eq!(
            dangerous_extension("https://github.com/o/r/blob/main/run.bat"),
            None
        );
        assert_eq!(
            dangerous_extension("https://github.com/o/r/releases/download/v1/app.exe").as_deref(),
            Some("exe")
        );
        assert_eq!(
            dangerous_extension("https://gitlab.com/o/r/-/downloads/app.zip").as_deref(),
            Some("zip")
        );
    }
}
