//! `crawl <click|right|hover> <out dir> [from to] [--nav steps separated by |]`: uses the app to find controls that do
//! nothing. Every element that asked for a pointing-hand cursor in the last frame (and that the pointer can reach at its
//! centre) is clicked, right-clicked or hovered in turn; the app's state is fingerprinted before and after.
//!
//! Classes: `changed` (what is drawn, or the app's described state, differs), `acted` (an inert refusal or skip was
//! logged), `read` (only a read request went out) and `dead` (nothing differs). `selected` is a dead element that
//! already carries a translucent fill (the current row or tab) and `decor` an element thinner than 4 px (a separator);
//! neither counts as a fault. Every result line names the element: its `parity` id (the best match among the recorded
//! ids, `~` when only roughly) and the text painted on or next to it. A dead element's crop (with the element outlined)
//! is saved, and `<mode>-screen.png` shows every dead element outlined on the whole frame. Each element is written to
//! `progress.txt` in the output directory before and after it is tried, so a crawl that dies shows where.
//! Runs only with `GRAVECORD_INERT=1`, so a click on Delete or Leave cannot reach the outside world.

use super::{dispatch, down, key, moved, right_down, up};
use crate::ui::Root;
use gpui::{AnyWindowHandle, AsyncApp, Bounds, Entity, Keystroke, Pixels, point, px};
use std::hash::{Hash, Hasher};
use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, PartialEq)]
struct Print {
    scene: u64,
    state: u64,
    acted: u64,
    reads: u64,
    /// What is open, bottom to top: a popout or menu opening shows here.
    layers: String,
}

fn hash(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

async fn settle(cx: &mut AsyncApp, window: AnyWindowHandle, ms: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(ms))
        .await;
    cx.update(|cx| {
        let _ = window.update(cx, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    });
}

fn print(cx: &mut AsyncApp, window: AnyWindowHandle, root: &Entity<Root>) -> Print {
    let state = root.read_with(cx, |root, cx| root.describe(cx));
    // The notification event list grows by itself; what follows it is the app's state.
    let state = state
        .split_once(" nameplates=")
        .map_or(state.clone(), |(_, rest)| rest.to_owned());
    let scene = cx
        .update(|cx| window.update(cx, |_, window, _| window.scene_fingerprint(px(34.))))
        .unwrap_or(0);
    Print {
        scene,
        state: hash(&state),
        acted: crate::inert::count(),
        reads: crate::inert::requests(),
        layers: root.read_with(cx, |root, _| root.crawl_layers()),
    }
}

/// One pointer element of the frame, with what names it.
struct Target {
    bounds: Bounds<Pixels>,
    /// Best-matching `parity` id.
    id: String,
    /// The text painted on or next to it.
    text: String,
    /// Painted text lies inside the bounds (false: icon only).
    has_text: bool,
    /// Already carries a translucent fill: the current selection.
    selected: bool,
}

fn iou(a: [f32; 4], b: [f32; 4]) -> f32 {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = (a[0] + a[2]).min(b[0] + b[2]);
    let y1 = (a[1] + a[3]).min(b[1] + b[3]);
    let inter = (x1 - x0).max(0.) * (y1 - y0).max(0.);
    let union = a[2] * a[3] + b[2] * b[3] - inter;
    if union <= 0. { 0. } else { inter / union }
}

fn clip(text: &str) -> String {
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut short: String = text.chars().take(40).collect();
    if text.chars().count() > 40 {
        short.push('~');
    }
    short.replace('"', "'")
}

/// Redraws the window with the parity and text logs empty, and returns the pointer elements named from them.
fn targets(cx: &mut AsyncApp, window: AnyWindowHandle) -> Vec<Target> {
    cx.update(|cx| {
        window
            .update(cx, |_, window, cx| {
                crate::parity::clear();
                gpui::inert_text_clear();
                window.refresh();
                window.draw(cx).clear(cx);
                let ids = crate::parity::snapshot();
                let texts: Vec<_> = gpui::inert_text_log()
                    .into_iter()
                    .filter(|(at, _)| window.text_visible(*at))
                    .collect();
                let mut found = window.pointer_hitboxes();
                // The title bar's window buttons stay out; a full-window backdrop is not a control.
                found.retain(|(bounds, _)| {
                    !(bounds.origin.y < px(34.) && bounds.origin.x > px(1100.))
                        && bounds.size.width < px(1200.)
                });
                found.sort_by(|a, b| {
                    (a.0.origin.y, a.0.origin.x)
                        .partial_cmp(&(b.0.origin.y, b.0.origin.x))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                found
                    .into_iter()
                    .map(|(bounds, _)| {
                        let own = [
                            f32::from(bounds.origin.x),
                            f32::from(bounds.origin.y),
                            f32::from(bounds.size.width),
                            f32::from(bounds.size.height),
                        ];
                        let best = ids
                            .iter()
                            .map(|(id, rect)| (id, iou(own, *rect)))
                            .filter(|(_, score)| *score > 0.3)
                            .max_by(|a, b| a.1.total_cmp(&b.1));
                        let id = match best {
                            Some((id, score)) if score >= 0.6 => id.clone(),
                            Some((id, _)) => format!("~{id}"),
                            None => {
                                // The smallest recorded element that contains it.
                                let centre = (own[0] + own[2] / 2., own[1] + own[3] / 2.);
                                ids.iter()
                                    .filter(|(_, r)| {
                                        centre.0 >= r[0]
                                            && centre.0 <= r[0] + r[2]
                                            && centre.1 >= r[1]
                                            && centre.1 <= r[1] + r[3]
                                    })
                                    .min_by(|a, b| (a.1[2] * a.1[3]).total_cmp(&(b.1[2] * b.1[3])))
                                    .map_or(String::new(), |(id, _)| format!("in:{id}"))
                            }
                        };
                        let inflated = bounds.dilate(px(2.));
                        let mut inside: Vec<&(Bounds<Pixels>, gpui::SharedString)> = texts
                            .iter()
                            .filter(|(at, _)| inflated.contains(&at.center()))
                            .collect();
                        inside.sort_by(|a, b| {
                            (a.0.origin.y, a.0.origin.x)
                                .partial_cmp(&(b.0.origin.y, b.0.origin.x))
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                        let has_text = !inside.is_empty();
                        let text = if has_text {
                            inside
                                .iter()
                                .take(3)
                                .map(|(_, text)| clip(text))
                                .collect::<Vec<_>>()
                                .join(" / ")
                        } else {
                            // The nearest text within 80 px (a label beside an icon button).
                            let c = bounds.center();
                            texts
                                .iter()
                                .map(|(at, text)| {
                                    let d = at.center();
                                    let dist = (f32::from(d.x - c.x).powi(2)
                                        + f32::from(d.y - c.y).powi(2))
                                    .sqrt();
                                    (dist, text)
                                })
                                .filter(|(dist, _)| *dist < 80.)
                                .min_by(|a, b| a.0.total_cmp(&b.0))
                                .map_or(String::new(), |(_, text)| format!("near:{}", clip(text)))
                        };
                        // A translucent fill over most of the element before any hover: the current selection.
                        let selected = window
                            .quad_fills(bounds, 0.85)
                            .iter()
                            .any(|alpha| *alpha < 0.95);
                        Target {
                            bounds,
                            id,
                            text,
                            has_text,
                            selected,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// The text visible on the screen (what a failed `--expect` could have named), top to bottom.
fn visible_text(cx: &mut AsyncApp, window: AnyWindowHandle) -> String {
    cx.update(|cx| {
        window
            .update(cx, |_, window, _| {
                let mut shown: Vec<_> = gpui::inert_text_log()
                    .into_iter()
                    .filter(|(at, _)| window.text_visible(*at))
                    .collect();
                shown.sort_by(|a, b| {
                    (a.0.origin.y, a.0.origin.x)
                        .partial_cmp(&(b.0.origin.y, b.0.origin.x))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                shown
                    .iter()
                    .take(80)
                    .map(|(_, text)| clip(text))
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .unwrap_or_default()
    })
}

/// Whether a parity id contains `needle` or visible text does.
fn shows(cx: &mut AsyncApp, window: AnyWindowHandle, needle: &str) -> bool {
    cx.update(|cx| {
        window
            .update(cx, |_, window, cx| {
                crate::parity::clear();
                gpui::inert_text_clear();
                window.refresh();
                window.draw(cx).clear(cx);
                crate::parity::snapshot()
                    .iter()
                    .any(|(id, _)| id.contains(needle))
                    || gpui::inert_text_log()
                        .iter()
                        .any(|(at, text)| text.contains(needle) && window.text_visible(*at))
            })
            .unwrap_or(false)
    })
}

fn frame(cx: &mut AsyncApp, window: AnyWindowHandle) -> Option<image::RgbaImage> {
    cx.update(|cx| {
        window
            .update(cx, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
                let size = window.viewport_size();
                window.render_to_image().ok().map(|image| (size, image))
            })
            .ok()
            .flatten()
    })
    .map(|(size, image)| {
        // Frames come back in physical pixels; the crawl works in logical ones.
        let scale = image.width() as f32 / f32::from(size.width).max(1.);
        if (scale - 1.).abs() < 0.01 {
            image
        } else {
            let width = (image.width() as f32 / scale).round() as u32;
            let height = (image.height() as f32 / scale).round() as u32;
            image::imageops::resize(&image, width, height, image::imageops::FilterType::Triangle)
        }
    })
}

fn outline(image: &mut image::RgbaImage, x: i64, y: i64, w: i64, h: i64) {
    let magenta = image::Rgba([255, 0, 255, 255]);
    let (iw, ih) = (i64::from(image.width()), i64::from(image.height()));
    for i in 0..w {
        for edge in [y, y + h - 1] {
            if (0..iw).contains(&(x + i)) && (0..ih).contains(&edge) {
                image.put_pixel((x + i) as u32, edge as u32, magenta);
            }
        }
    }
    for j in 0..h {
        for edge in [x, x + w - 1] {
            if (0..iw).contains(&edge) && (0..ih).contains(&(y + j)) {
                image.put_pixel(edge as u32, (y + j) as u32, magenta);
            }
        }
    }
}

fn whole(bounds: Bounds<Pixels>) -> (i64, i64, i64, i64) {
    (
        f32::from(bounds.origin.x).round() as i64,
        f32::from(bounds.origin.y).round() as i64,
        (f32::from(bounds.size.width).round() as i64).max(1),
        (f32::from(bounds.size.height).round() as i64).max(1),
    )
}

/// The element with a margin round it, taken from the frame drawn before the click, outlined and (when small) enlarged.
fn crop(base: &image::RgbaImage, bounds: Bounds<Pixels>, path: &Path) {
    let margin = 10i64;
    let (ex, ey, ew, eh) = whole(bounds);
    let x0 = (ex - margin).max(0);
    let y0 = (ey - margin).max(0);
    let x1 = (ex + ew + margin).min(i64::from(base.width()));
    let y1 = (ey + eh + margin).min(i64::from(base.height()));
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let mut cropped = image::imageops::crop_imm(
        base,
        x0 as u32,
        y0 as u32,
        (x1 - x0) as u32,
        (y1 - y0) as u32,
    )
    .to_image();
    outline(&mut cropped, ex - x0 - 1, ey - y0 - 1, ew + 2, eh + 2);
    if cropped.width() < 160 {
        let k = (160 / cropped.width().max(1)).clamp(2, 4);
        cropped = image::imageops::resize(
            &cropped,
            cropped.width() * k,
            cropped.height() * k,
            image::imageops::FilterType::Nearest,
        );
    }
    let _ = cropped.save(path);
}

/// Appends one line to the output string and to `progress.txt` at once.
fn log(out: &mut String, dir: &Path, line: &str) {
    out.push_str(line);
    out.push('\n');
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("progress.txt"))
    {
        let _ = writeln!(file, "{line}");
    }
}

pub async fn run(
    words: &[&str],
    window: AnyWindowHandle,
    root: &Entity<Root>,
    cx: &mut AsyncApp,
) -> String {
    let mut out = String::new();
    if !crate::inert::on() {
        return "crawl: refused, GRAVECORD_INERT=1 is not set\n".to_owned();
    }
    // `--nav a | b | c` after the other words: steps that bring the app back to where the crawl started.
    // `--neutral x y`: a blank place to click so that focus is the same before and after (0 0: none).
    let mut neutral = Some((1255., 360.));
    // `--expect text_or_id`: a parity id or visible text (`_` is a space) the screen must show, before and after each element.
    let mut expect: Option<String> = None;
    // `--rest x y`: where the pointer waits between elements (default: the bottom left corner, over nothing). A page whose
    // controls show only under the pointer (a message's toolbar) rests on the row that carries them.
    let mut rest = point(px(2.), px(715.));
    // `--only text`: crawl only the elements whose parity id contains the text; `--within x0 y0 x1 y1`: only those whose
    // centre lies in the rectangle. A popout leaves the page under it pressable, and the first press outside closes it.
    let mut only: Option<String> = None;
    let mut within: Option<[f32; 4]> = None;
    // `--dedupe n`: of the elements with the same id and size (the cells of an emoji grid) only the first `n` are tried.
    let mut dedupe: Option<usize> = None;
    let mut kept: Vec<&str> = Vec::new();
    let mut skip = 0;
    for (at, word) in words.iter().enumerate() {
        if skip > 0 {
            skip -= 1;
        } else if *word == "--neutral" && words.len() > at + 2 {
            let spot: Option<(f32, f32)> =
                words[at + 1].parse().ok().zip(words[at + 2].parse().ok());
            neutral = spot.filter(|spot| spot.0 > 0.);
            skip = 2;
        } else if *word == "--only" && words.len() > at + 1 {
            only = Some(words[at + 1].to_owned());
            skip = 1;
        } else if *word == "--dedupe" && words.len() > at + 1 {
            dedupe = words[at + 1].parse().ok();
            skip = 1;
        } else if *word == "--within" && words.len() > at + 4 {
            let numbers: Vec<f32> = words[at + 1..at + 5]
                .iter()
                .filter_map(|word| word.parse().ok())
                .collect();
            if let Ok(rect) = <[f32; 4]>::try_from(numbers) {
                within = Some(rect);
            }
            skip = 4;
        } else if *word == "--rest" && words.len() > at + 2 {
            if let (Ok(x), Ok(y)) = (words[at + 1].parse::<f32>(), words[at + 2].parse::<f32>()) {
                rest = point(px(x), px(y));
            }
            skip = 2;
        } else if *word == "--expect" && words.len() > at + 1 {
            expect = Some(words[at + 1].replace('_', " "));
            skip = 1;
        } else {
            kept.push(word);
        }
    }
    let words: &[&str] = &kept;
    let (words, nav): (&[&str], String) = match words.iter().position(|word| *word == "--nav") {
        Some(at) => (&words[..at], words[at + 1..].join(" ").replace('|', ";")),
        None => (words, String::new()),
    };
    let [mode, dir, tail @ ..] = words else {
        return "usage: crawl <click|right|hover> <out dir> [from to]\n".to_owned();
    };
    let dir = Path::new(dir);
    let _ = std::fs::create_dir_all(dir);
    let range: Option<(usize, usize)> = match tail {
        [from, to] => from.parse().ok().zip(to.parse().ok()),
        _ => None,
    };
    let blur = |cx: &mut AsyncApp| {
        if let Some((x, y)) = neutral {
            let spot = point(px(x), px(y));
            cx.update(|cx| {
                dispatch(cx, window, moved(spot, None));
                dispatch(cx, window, down(spot));
                dispatch(cx, window, up(spot));
            });
        }
    };
    blur(cx);
    cx.update(|cx| dispatch(cx, window, moved(rest, None)));
    settle(cx, window, 400).await;
    let _ = std::fs::remove_file(dir.join("progress.txt"));
    // A slow machine paints late: look for up to three seconds before calling the setup failed.
    let mut found = false;
    if let Some(needle) = &expect {
        for _ in 0..6 {
            if shows(cx, window, needle) {
                found = true;
                break;
            }
            settle(cx, window, 500).await;
        }
    }
    if let Some(needle) = &expect
        && !found
    {
        if let Some(image) = frame(cx, window) {
            let _ = image.save(dir.join("setup-failed.png"));
        }
        let seen = visible_text(cx, window);
        log(
            &mut out,
            dir,
            &format!("crawl: setup failed, `{needle}` is not on the screen; text seen: {seen}"),
        );
        return out;
    }
    let mut list = targets(cx, window);
    list.retain(|target| {
        let centre = target.bounds.center();
        only.as_ref()
            .is_none_or(|text| target.id.contains(text.as_str()))
            && within.is_none_or(|[x0, y0, x1, y1]| {
                (x0..=x1).contains(&f32::from(centre.x)) && (y0..=y1).contains(&f32::from(centre.y))
            })
    });
    if let Some(keep) = dedupe {
        let mut seen: std::collections::HashMap<(String, i64, i64), usize> = Default::default();
        list.retain(|target| {
            let (_, _, w, h) = whole(target.bounds);
            let count = seen.entry((target.id.clone(), w, h)).or_default();
            *count += 1;
            *count <= keep
        });
    }
    let base_texts: std::collections::HashSet<String> = cx.update(|cx| {
        window
            .update(cx, |_, window, _| {
                gpui::inert_text_log()
                    .into_iter()
                    .filter(|(at, _)| window.text_visible(*at))
                    .map(|(_, text)| text.to_string())
                    .collect()
            })
            .unwrap_or_default()
    });
    let base = frame(cx, window);
    let total = list.len();
    let mut start = print(cx, window, root);
    // Is what is drawn steady by itself? A spinner or a blinking caret makes the scene differ with no input.
    settle(cx, window, 300).await;
    let steady = print(cx, window, root).scene == start.scene;
    let mut counts: std::collections::BTreeMap<&'static str, usize> = Default::default();
    let mut visited = 0;
    // Set when the app could not be brought back: later elements may no longer be where the list says.
    let mut drifted = false;
    let mut dead_boxes: Vec<Bounds<Pixels>> = Vec::new();
    log(
        &mut out,
        dir,
        &format!("crawl {mode}: {total} pointer elements, scene steady: {steady}"),
    );
    for (index, target) in list.iter().enumerate() {
        let bounds = &target.bounds;
        if let Some((from, to)) = range
            && !(from..to).contains(&index)
        {
            continue;
        }
        visited += 1;
        let centre = point(
            bounds.origin.x + bounds.size.width / 2.,
            bounds.origin.y + bounds.size.height / 2.,
        );
        let name = format!(
            "bounds=({:.0},{:.0} {:.0}x{:.0}) id={} text=\"{}\"",
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
            if target.id.is_empty() {
                "-"
            } else {
                &target.id
            },
            target.text,
        );
        // A hit box thinner than 4 px is a separator or a resize edge, not a control.
        if bounds.size.width.min(bounds.size.height) < px(4.) {
            *counts.entry("decor").or_default() += 1;
            log(&mut out, dir, &format!("#{index} decor {name}"));
            continue;
        }
        log(&mut out, dir, &format!(">> #{index} {name}"));
        let before = print(cx, window, root);
        let lines_from = crate::inert::line_count();
        let mut class: &'static str;
        let mut tooltip = String::new();
        match *mode {
            "hover" => {
                cx.update(|cx| dispatch(cx, window, moved(centre, None)));
                settle(cx, window, 120).await;
                let early = print(cx, window, root).scene;
                settle(cx, window, 1100).await;
                let late = print(cx, window, root).scene;
                let shown: Vec<String> = cx.update(|cx| {
                    window
                        .update(cx, |_, window, cx| {
                            gpui::inert_text_clear();
                            window.refresh();
                            window.draw(cx).clear(cx);
                            gpui::inert_text_log()
                                .into_iter()
                                .filter(|(at, _)| window.text_visible(*at))
                                .map(|(_, text)| text.to_string())
                                .filter(|text| !base_texts.contains(text))
                                .collect()
                        })
                        .unwrap_or_default()
                });
                tooltip = shown
                    .iter()
                    .map(|text| clip(text))
                    .collect::<Vec<_>>()
                    .join(" / ");
                class = if early != late || !shown.is_empty() {
                    "tooltip"
                } else if target.has_text {
                    // A labelled control needs no tooltip.
                    "labelled"
                } else {
                    "no-tooltip"
                };
                cx.update(|cx| dispatch(cx, window, moved(rest, None)));
            }
            other => {
                cx.update(|cx| {
                    dispatch(cx, window, moved(centre, None));
                    if other == "right" {
                        dispatch(cx, window, right_down(centre));
                    } else {
                        dispatch(cx, window, down(centre));
                        dispatch(cx, window, up(centre));
                    }
                });
                settle(cx, window, 250).await;
                settle(cx, window, 100).await;
                let after = print(cx, window, root);
                // `acted`: an inert refusal or skip was logged. `read`: only a read request went out, with nothing
                // else different (a control that loads data).
                class = if after.acted != before.acted {
                    "acted"
                } else if other == "right" {
                    // A right press is wired when it opens a menu or popout; the pointer moving changes the rest.
                    if after.layers != before.layers {
                        "changed"
                    } else {
                        "dead"
                    }
                } else if after.state != before.state || (steady && after.scene != before.scene) {
                    "changed"
                } else if after.reads != before.reads {
                    "read"
                } else {
                    "dead"
                };
                if class == "dead" && other != "right" {
                    // A control that waits for the network shows its result later: look again before calling it dead.
                    settle(cx, window, 1200).await;
                    let late = print(cx, window, root);
                    if late.acted != before.acted {
                        class = "acted";
                    } else if late.state != before.state || (steady && late.scene != before.scene) {
                        class = "changed";
                    } else if late.reads != before.reads {
                        class = "read";
                    }
                }
                if class == "dead"
                    && let Some(image) = frame(cx, window)
                {
                    // What the page looks like after the click that did nothing.
                    let _ = image.save(dir.join(format!("{mode}-{index}-after.png")));
                }
                if class == "dead" && target.selected {
                    class = "selected";
                }
            }
        }
        *counts.entry(class).or_default() += 1;
        let notes = crate::inert::lines_since(lines_from).join(" | ");
        let extra = if tooltip.is_empty() {
            String::new()
        } else {
            format!(" tooltip=\"{tooltip}\"")
        };
        log(
            &mut out,
            dir,
            &format!(
                "#{index} {class} {name}{extra}{} {notes}",
                if drifted { " (page drifted)" } else { "" }
            ),
        );
        if class == "dead" || class == "no-tooltip" || class == "selected" {
            if let Some(base) = &base {
                crop(
                    base,
                    *bounds,
                    &dir.join(format!("{mode}-{index}-{class}.png")),
                );
            }
            if class != "selected" {
                dead_boxes.push(*bounds);
            }
        }
        // Back to the start: Escape twice, then the steps that re-open the screen (a click that closed the whole
        // page, such as a settings page's close button, is undone by them), or else a second press.
        let mut tries = 0;
        let mut navved = false;
        loop {
            blur(cx);
            cx.update(|cx| dispatch(cx, window, moved(rest, None)));
            settle(cx, window, 150).await;
            let now = print(cx, window, root);
            if now.state == start.state && (!steady || now.scene == start.scene) {
                break;
            }
            if (tries >= 5 || navved) && (now.state == start.state || now.scene == start.scene) {
                // The same page (the state, or else the picture, matches): what differs is a focus ring, a scroll offset, a hover, or the
                // navigation history the state describes.
                start = now;
                break;
            }
            if tries >= 5 || navved {
                // Not back where it started: say so, take this as the new start and go on.
                log(
                    &mut out,
                    dir,
                    &format!(
                        "crawl: not back at the start after #{index} (state same: {}, scene same: {}); new baseline",
                        now.state == start.state,
                        now.scene == start.scene
                    ),
                );
                start = now;
                drifted = true;
                break;
            }
            if tries == 2 && !nav.is_empty() {
                let log_path = dir.join("nav.log");
                Box::pin(super::run(&nav, window, root.clone(), log_path, cx)).await;
                settle(cx, window, 900).await;
                navved = true;
                continue;
            }
            if tries == 2 && *mode != "hover" {
                // A toggle (members list, search) goes back with a second press.
                cx.update(|cx| {
                    dispatch(cx, window, moved(centre, None));
                    dispatch(cx, window, down(centre));
                    dispatch(cx, window, up(centre));
                });
                settle(cx, window, 300).await;
                tries += 1;
                continue;
            }
            cx.update(|cx| {
                if let Ok(stroke) = Keystroke::parse("escape") {
                    key(cx, window, stroke);
                }
            });
            settle(cx, window, 200).await;
            tries += 1;
        }
        if let Some(needle) = &expect
            && !shows(cx, window, needle)
        {
            // The screen is gone (a click left the page): the re-open steps, once; if it still is not there, stop.
            if !nav.is_empty() {
                let log_path = dir.join("nav.log");
                Box::pin(super::run(&nav, window, root.clone(), log_path, cx)).await;
                settle(cx, window, 900).await;
            }
            if !shows(cx, window, needle) {
                log(
                    &mut out,
                    dir,
                    &format!(
                        "crawl: lost the screen after #{index}, `{needle}` is not shown; stopped"
                    ),
                );
                break;
            }
            start = print(cx, window, root);
        }
    }
    if let Some(needle) = &expect
        && !shows(cx, window, needle)
    {
        log(
            &mut out,
            dir,
            &format!("crawl: lost the screen, `{needle}` is gone at the end"),
        );
    }
    if let Some(mut whole_frame) = base
        && !dead_boxes.is_empty()
    {
        for bounds in &dead_boxes {
            let (x, y, w, h) = whole(*bounds);
            outline(&mut whole_frame, x - 1, y - 1, w + 2, h + 2);
        }
        let _ = whole_frame.save(dir.join(format!("{mode}-screen.png")));
    }
    let tally: Vec<String> = counts
        .iter()
        .map(|(class, count)| format!("{class} {count}"))
        .collect();
    log(
        &mut out,
        dir,
        &format!(
            "crawl summary: visited {visited} of {total} elements {total} {}",
            tally.join(" ")
        ),
    );
    out
}
