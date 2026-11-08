//! Which displays and windows the picker lists: the pure rules, apart from the OS calls that find them.
//!
//! Bounds and name trimming follow Serein's `screen/capture.rs` and `capture_windows.rs` (MIT OR Apache-2.0). Serein
//! lists every titled window `windows-capture` enumerates; this also drops this process's own windows, minimized and
//! cloaked ones (nothing can be captured from them) and tool windows.

use super::{Source, SourceId};
use crate::Picture;

pub(crate) const MAX_SOURCES: usize = 64;
pub(crate) const MAX_NAME_BYTES: usize = 256;
/// Sources larger than 8K are not offered.
pub(crate) const MAX_SOURCE_WIDTH: u32 = 7680;
pub(crate) const MAX_SOURCE_HEIGHT: u32 = 4320;

/// What the OS says about one top-level window.
#[derive(Clone, Debug, Default)]
pub(crate) struct WindowInfo {
    pub hwnd: u64,
    pub title: String,
    pub class: String,
    pub pid: u32,
    pub width: u32,
    pub height: u32,
    pub visible: bool,
    pub minimized: bool,
    pub cloaked: bool,
    /// `WS_EX_TOOLWINDOW` without `WS_EX_APPWINDOW`: palettes, tooltips and the like.
    pub tool_window: bool,
    pub refresh_hz: u32,
}

/// One display as the OS reports it, with its real pixel size.
#[derive(Clone, Debug, Default)]
pub(crate) struct DisplayInfo {
    pub handle: u64,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
}

/// Window classes of the shell itself: the desktop and the taskbar are not something to share.
const SHELL_CLASSES: [&str; 4] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

/// Whether a window belongs in the picker: visible, titled, shown on screen, not ours.
pub(crate) fn keep_window(window: &WindowInfo, own_pid: u32) -> bool {
    window.hwnd != 0
        && window.pid != own_pid
        && window.visible
        && !window.minimized
        && !window.cloaked
        && !window.tool_window
        && !window.title.trim().is_empty()
        && !SHELL_CLASSES.contains(&window.class.as_str())
        && window.width > 0
        && window.height > 0
        && window.width <= MAX_SOURCE_WIDTH
        && window.height <= MAX_SOURCE_HEIGHT
}

/// Cuts `name` to [`MAX_NAME_BYTES`] on a character boundary.
pub(crate) fn bounded_name(mut name: String) -> String {
    if name.len() > MAX_NAME_BYTES {
        let mut end = MAX_NAME_BYTES;
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        name.truncate(end);
        name.shrink_to_fit();
    }
    name
}

/// The picker list: displays first, then the windows that pass [`keep_window`], at most [`MAX_SOURCES`] in all.
/// `icon` supplies a window's icon.
pub(crate) fn assemble(
    displays: Vec<DisplayInfo>,
    windows: Vec<WindowInfo>,
    own_pid: u32,
    mut icon: impl FnMut(u64) -> Option<Picture>,
) -> Vec<Source> {
    let mut sources = Vec::new();
    for display in displays {
        if display.width == 0
            || display.height == 0
            || display.width > MAX_SOURCE_WIDTH
            || display.height > MAX_SOURCE_HEIGHT
        {
            continue;
        }
        if sources.len() == MAX_SOURCES {
            return sources;
        }
        sources.push(Source {
            id: SourceId::Display(display.handle),
            name: bounded_name(display.name),
            width: display.width,
            height: display.height,
            refresh_hz: display.refresh_hz,
            icon: None,
        });
    }
    for window in windows {
        if !keep_window(&window, own_pid) {
            continue;
        }
        if sources.len() == MAX_SOURCES {
            break;
        }
        sources.push(Source {
            id: SourceId::Window(window.hwnd),
            icon: icon(window.hwnd),
            name: bounded_name(window.title),
            width: window.width,
            height: window.height,
            refresh_hz: window.refresh_hz,
        });
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: u32 = 4242;

    fn window(hwnd: u64, title: &str) -> WindowInfo {
        WindowInfo {
            hwnd,
            title: title.into(),
            class: "Chrome_WidgetWin_1".into(),
            pid: 100,
            width: 1280,
            height: 720,
            visible: true,
            refresh_hz: 60,
            ..Default::default()
        }
    }

    #[test]
    fn plain_titled_windows_are_kept() {
        assert!(keep_window(&window(1, "Notes"), OWN));
    }

    #[test]
    fn our_own_windows_are_skipped() {
        let own = WindowInfo {
            pid: OWN,
            ..window(2, "gravecord")
        };
        assert!(!keep_window(&own, OWN));
    }

    #[test]
    fn untitled_hidden_minimized_cloaked_tool_and_shell_windows_are_skipped() {
        for bad in [
            window(3, ""),
            window(3, "   \t"),
            WindowInfo {
                visible: false,
                ..window(3, "x")
            },
            WindowInfo {
                minimized: true,
                ..window(3, "x")
            },
            WindowInfo {
                cloaked: true,
                ..window(3, "x")
            },
            WindowInfo {
                tool_window: true,
                ..window(3, "x")
            },
            WindowInfo {
                class: "Progman".into(),
                ..window(3, "Program Manager")
            },
            WindowInfo {
                class: "Shell_TrayWnd".into(),
                ..window(3, "tray")
            },
            WindowInfo {
                width: 0,
                ..window(3, "x")
            },
            WindowInfo {
                height: 0,
                ..window(3, "x")
            },
            WindowInfo {
                width: MAX_SOURCE_WIDTH + 1,
                ..window(3, "x")
            },
            WindowInfo {
                height: MAX_SOURCE_HEIGHT + 1,
                ..window(3, "x")
            },
            window(0, "no handle"),
        ] {
            assert!(!keep_window(&bad, OWN), "{bad:?}");
        }
    }

    #[test]
    fn names_end_on_utf8_boundaries() {
        let name = bounded_name(format!("{}\u{e9}", "a".repeat(MAX_NAME_BYTES - 1)));
        assert_eq!(name.len(), MAX_NAME_BYTES - 1);
        assert_eq!(bounded_name("short".into()), "short");
        // A run of three-byte characters is cut between characters, never inside one.
        let wide = bounded_name("\u{20ac}".repeat(200));
        assert!(wide.len() <= MAX_NAME_BYTES && wide.len().is_multiple_of(3));
    }

    #[test]
    fn displays_come_first_and_the_list_is_bounded() {
        let displays = vec![
            DisplayInfo {
                handle: 10,
                name: "Display 1".into(),
                width: 2560,
                height: 1440,
                refresh_hz: 144,
            },
            DisplayInfo {
                handle: 11,
                name: "gone".into(),
                width: 0,
                height: 0,
                refresh_hz: 0,
            },
            DisplayInfo {
                handle: 12,
                name: "Display 2".into(),
                width: 1920,
                height: 1080,
                refresh_hz: 60,
            },
        ];
        let windows = vec![
            window(1, "Notes"),
            WindowInfo {
                pid: OWN,
                ..window(2, "ours")
            },
            window(3, "Browser"),
        ];
        let sources = assemble(displays, windows, OWN, |hwnd| {
            (hwnd == 3).then(|| Picture::rgba(1, 1, vec![1, 2, 3, 255]).unwrap())
        });
        let ids: Vec<SourceId> = sources.iter().map(|s| s.id).collect();
        assert_eq!(
            ids,
            [
                SourceId::Display(10),
                SourceId::Display(12),
                SourceId::Window(1),
                SourceId::Window(3)
            ]
        );
        assert_eq!(sources[0].refresh_hz, 144);
        assert!(sources[2].icon.is_none() && sources[3].icon.is_some());

        let many: Vec<WindowInfo> = (1..=200).map(|i| window(i, "w")).collect();
        let sources = assemble(Vec::new(), many, OWN, |_| None);
        assert_eq!(sources.len(), MAX_SOURCES);
        // Displays are never crowded out by windows.
        let displays: Vec<DisplayInfo> = (1..=3)
            .map(|i| DisplayInfo {
                handle: i,
                name: "d".into(),
                width: 100,
                height: 100,
                refresh_hz: 60,
            })
            .collect();
        let many: Vec<WindowInfo> = (1..=200).map(|i| window(i, "w")).collect();
        let sources = assemble(displays, many, OWN, |_| None);
        assert!(matches!(sources[2].id, SourceId::Display(3)));
        assert_eq!(sources.len(), MAX_SOURCES);
    }
}
