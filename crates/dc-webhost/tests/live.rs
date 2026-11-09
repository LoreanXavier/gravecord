//! The captcha page and the relay against a real WebView2, on a hidden owner window. Ignored by default: it needs
//! WebView2 and the network, and it must run on the hidden desktop (`cargo xtask headless <test exe> --ignored`), never
//! on the owner's.
//!
//! No captcha is solved here. The widget is rendered with hCaptcha's published test sitekey and left alone (nothing
//! clicks it), and the relay is shown to pass a fake token that the test's own page posts.

#![cfg(windows)]

use dc_webhost::{CaptchaEvent, HostEvent, WebHost, captcha_event};
use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use std::num::NonZeroIsize;
use std::time::{Duration, Instant};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
    TranslateMessage, WS_OVERLAPPED,
};
use windows::core::w;

/// A window that is never shown: the surfaces need an owner.
struct Owner(windows::Win32::Foundation::HWND);

impl Owner {
    fn new() -> Self {
        // SAFETY: a plain STATIC window on this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("owner"),
                WS_OVERLAPPED,
                0,
                0,
                400,
                300,
                None,
                None,
                None,
                None,
            )
        }
        .expect("owner window");
        Self(hwnd)
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        // SAFETY: made on this thread.
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

impl HasWindowHandle for Owner {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = Win32WindowHandle::new(NonZeroIsize::new(self.0.0 as isize).expect("handle"));
        // SAFETY: the window lives as long as the owner.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

/// Progress lines go to the file named by `GRAVECORD_TEST_LOG` too: on the hidden desktop the console output is lost.
fn note(line: &str) {
    eprintln!("step: {line}");
    if let Some(path) = std::env::var_os("GRAVECORD_TEST_LOG") {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{line}");
        }
    }
}

/// The owner's thread must keep pumping: the surface's popup is owned by this window, and showing it sends messages here.
fn pump_owner() {
    let mut message = MSG::default();
    // SAFETY: plain message loop calls on the thread that made the owner window.
    unsafe {
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn wait_for(
    host: &WebHost,
    within: Duration,
    want: impl Fn(&HostEvent) -> bool,
) -> Option<HostEvent> {
    let start = Instant::now();
    while start.elapsed() < within {
        pump_owner();
        for event in host.poll() {
            note(&format!("event: {event:?}"));
            if let HostEvent::Failed(why) = &event {
                panic!("the surface failed: {why}");
            }
            if want(&event) {
                return Some(event);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
#[ignore = "needs WebView2 and network; run on the hidden desktop"]
fn the_captcha_page_loads_and_the_relay_passes_a_fake_token() {
    let owner = Owner::new();
    note("owner window made");
    // WebView2's setup can wait on the owner's thread for good on a desktop it cannot use: end the run, never hang.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(120));
        note("watchdog: the surface never came up");
        std::process::exit(3);
    });
    let dir = std::env::temp_dir().join(format!("gravecord-webhost-test-{}", std::process::id()));

    // hCaptcha's public test sitekey. The widget renders; nobody clicks it.
    let host = WebHost::captcha(
        &owner,
        dir.join("captcha"),
        (0., 0., 302., 78.),
        "10000000-ffff-ffff-ffff-000000000001",
        Some("test-rqdata"),
    )
    .expect("captcha surface");
    note("captcha surface made");
    note("waiting for the widget");
    let rendered = wait_for(&host, Duration::from_secs(40), |event| {
        captcha_event(event) == Some(CaptchaEvent::Rendered)
    });
    if rendered.is_none() {
        // The surface thread may still be inside WebView2's setup: dropping the host would wait for it.
        note("the widget did not report in time");
        std::mem::forget(host);
        std::process::exit(2);
    }
    assert!(
        rendered.is_some(),
        "the widget page did not report a rendered widget"
    );
    assert!(
        host.poll()
            .iter()
            .all(|event| !matches!(captcha_event(event), Some(CaptchaEvent::Solved(_)))),
        "nothing may solve it"
    );
    note("widget rendered");
    drop(host);

    // The relay passes what a framed page posts, under that page's own origin, which is never the captcha origin.
    let relay =
        WebHost::new(&owner, dir.join("relay"), (0., 0., 300., 200.)).expect("relay surface");
    wait_for(&relay, Duration::from_secs(20), |event| {
        matches!(event, HostEvent::Ready)
    })
    .expect("relay ready");
    note("relay ready");
    relay
        .open("data:text/html,<script>parent.postMessage({token:'FAKE-RELAY-TOKEN'},'*')</script>");
    note("relay posted");
    let message = wait_for(&relay, Duration::from_secs(20), |event| {
        matches!(event, HostEvent::Message { .. })
    })
    .expect("the relay passed the message");
    let HostEvent::Message { data, .. } = &message else {
        unreachable!()
    };
    assert_eq!(data["token"], "FAKE-RELAY-TOKEN");
    assert_eq!(
        captcha_event(&message),
        None,
        "a framed page's message is not a captcha answer"
    );
    note("relay passed the fake token");
    let _ = std::fs::remove_dir_all(&dir);
}
