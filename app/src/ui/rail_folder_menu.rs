//! The folder's context menu (module 865815: Mark Folder As Read, Move to..., Folder Settings, Close All Folders) and
//! the Folder Settings modal (module 672551: name, twenty preset colours and a custom colour). Geometry comes from
//! `design/dom/rail/folder-menu-*.json` and `folder-settings*.json`; the Move to... submenu has no capture (one root
//! item hides it) and is built from the module.

use super::{Root, color, editor_style, icon, modal};
use crate::editor::{Editor, EditorEvent};
use gpui::{
    AnyElement, Bounds, Context, Entity, FontWeight, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, Point, SharedString, Subscription, Window, anchored, canvas, deferred,
    div, linear_color_stop, linear_gradient, prelude::*, px, rgba,
};
use std::cell::Cell;
use std::rc::Rc;

/// The folder menu: which folder, where the pointer was, and whether the Move to... submenu is out.
#[derive(Clone, Copy)]
pub(super) struct FolderMenu {
    pub folder: u64,
    pub at: Point<Pixels>,
    pub sub: bool,
}

/// `Folder Name` is capped at 32 characters (`maxLength` of the input, module 672551).
const NAME_MAX: usize = 32;

/// The preset swatches of the colour picker, in the order of the capture: two rows of ten (`ColorPicker` presets,
/// decimal ids `color-picker___preset-<value>`).
const PRESETS: [[u32; 10]; 2] = [
    [
        0x1abc9c, 0x2ecc71, 0x3498db, 0x9b59b6, 0xe91e63, 0xf1c40f, 0xe67e22, 0xe74c3c, 0x95a5a6,
        0x607d8b,
    ],
    [
        0x11806a, 0x1f8b4c, 0x206694, 0x71368a, 0xad1457, 0xc27c0e, 0xa84300, 0x992d22, 0x979c9f,
        0x546e7a,
    ],
];

/// The default folder colour (`--brand-500`, the swatch of the capture).
const DEFAULT_COLOUR: u32 = 0x5865f2;

pub(super) struct Picker {
    hue: f32,
    saturation: f32,
    value: f32,
    hex: Entity<Editor>,
    boxed: Rc<Cell<Bounds<Pixels>>>,
    slider: Rc<Cell<Bounds<Pixels>>>,
    _events: Subscription,
}

pub(super) struct FolderSettings {
    pub folder: u64,
    name: Entity<Editor>,
    color: Option<u64>,
    picker: Option<Picker>,
    _events: Subscription,
    /// Not in the pinned client: the dialog edits a PinDMs category instead of a folder (`plugin_hooks/pin_dms.rs`).
    plugin: Option<super::plugin_hooks::PinCategoryDraft>,
}

fn to_hsv(rgb: u32) -> (f32, f32, f32) {
    let r = ((rgb >> 16) & 0xff) as f32 / 255.;
    let g = ((rgb >> 8) & 0xff) as f32 / 255.;
    let b = (rgb & 0xff) as f32 / 255.;
    let max = r.max(g).max(b);
    let delta = max - r.min(g).min(b);
    let hue = if delta == 0. {
        0.
    } else if max == r {
        60. * (((g - b) / delta).rem_euclid(6.))
    } else if max == g {
        60. * ((b - r) / delta + 2.)
    } else {
        60. * ((r - g) / delta + 4.)
    };
    (hue, if max == 0. { 0. } else { delta / max }, max)
}

fn from_hsv(hue: f32, saturation: f32, value: f32) -> u32 {
    let chroma = value * saturation;
    let x = chroma * (1. - ((hue / 60.) % 2. - 1.).abs());
    let m = value - chroma;
    let (r, g, b) = match (hue / 60.).floor() as i32 {
        0 => (chroma, x, 0.),
        1 => (x, chroma, 0.),
        2 => (0., chroma, x),
        3 => (0., x, chroma),
        4 => (x, 0., chroma),
        _ => (chroma, 0., x),
    };
    let channel = |value: f32| ((value + m) * 255.).round().clamp(0., 255.) as u32;
    (channel(r) << 16) | (channel(g) << 8) | channel(b)
}

fn text_line(text: &'static str, weight: f32, tint: &'static str) -> gpui::Div {
    div()
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(weight))
        .text_color(color(tint))
        .child(text)
}

impl Root {
    /// Right click on a folder header: the menu opens with its corner at the pointer (capture: 36,113).
    pub(super) fn open_folder_menu(
        &mut self,
        folder: u64,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.close_all_context_menus();
        self.rail_folders.menu = Some(FolderMenu {
            folder,
            at,
            sub: false,
        });
        cx.notify();
    }

    /// The folder has an unread channel or a mention in one of its servers (`unread` of the folder item).
    fn folder_unread(&self, folder: u64) -> bool {
        self.stores.folders.folder(folder).is_some_and(|folder| {
            folder.guilds.iter().any(|id| {
                self.stores.guild(*id).is_some_and(|guild| {
                    let counts = self.guild_rail_counts(guild);
                    counts.unread || counts.mentions > 0
                })
            })
        })
    }

    /// "Mark Folder As Read": every server of the folder is acknowledged (`ackGuilds`, module 567035).
    fn mark_folder_read(&mut self, folder: u64, cx: &mut Context<Self>) {
        let guilds = self
            .stores
            .folders
            .folder(folder)
            .map(|folder| folder.guilds.clone())
            .unwrap_or_default();
        self.mark_guilds_read(&guilds, cx);
    }

    pub(super) fn folder_menu_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.rail_folders.menu?;
        let folder = open.folder;
        let unread = self.folder_unread(folder);
        let any_open = self.stores.folders.expanded().next().is_some();
        let placements = (self.stores.folders.roots().len() > 1)
            .then(|| self.stores.folders.placement_disabled(folder))
            .flatten();
        // One row: 34 high with 8px of padding, 14/18 medium text; hover is the interactive wash.
        let row = |id: &'static str, label: &'static str, disabled: bool| {
            div()
                .id(id)
                .h(px(34.))
                .flex_none()
                .p(px(8.))
                .rounded(px(2.))
                .flex()
                .items_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .whitespace_nowrap()
                .text_color(color(if disabled {
                    "--text-muted"
                } else {
                    "--text-strong"
                }))
                .when(!disabled, |this| {
                    this.cursor_pointer().hover(|style| {
                        style
                            .bg(color("--interactive-background-hover"))
                            .rounded(px(4.))
                    })
                })
                .child(label)
        };
        let separator = || {
            div()
                .my(px(8.))
                .h(px(1.))
                .flex_none()
                .bg(color("--border-subtle"))
        };
        let mut rows: Vec<AnyElement> = vec![
            row(
                "folder-menu-read",
                dc_intl::tr("Mark Folder As Read"),
                !unread,
            )
            .when(unread, |this| {
                this.on_click(cx.listener(move |this, _, _, cx| {
                    this.mark_folder_read(folder, cx);
                    this.rail_folders.menu = None;
                    cx.notify();
                }))
            })
            .into_any_element(),
            separator().into_any_element(),
        ];
        if let Some((first_disabled, last_disabled)) = placements {
            let submenu = open.sub;
            let sub_row = |id: &'static str, label: &'static str, disabled: bool, place| {
                row(id, label, disabled).when(!disabled, |this| {
                    this.on_click(cx.listener(move |this, _, _, cx| {
                        let patch = this.stores.move_guild(folder, place);
                        this.send_rail_patch(patch);
                        this.rail_folders.menu = None;
                        cx.notify();
                    }))
                })
            };
            rows.push(
                row("folder-menu-move", dc_intl::tr("Move to..."), false)
                    .relative()
                    .justify_between()
                    .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                        if *inside && let Some(menu) = this.rail_folders.menu.as_mut() {
                            menu.sub = true;
                            cx.notify();
                        }
                    }))
                    .child(icon("menu_caret", 18., "--interactive-text-default"))
                    .when(submenu, |this| {
                        this.child(
                            div()
                                .absolute()
                                .left(gpui::relative(1.))
                                .top(px(-9.))
                                .ml(px(8.))
                                .rounded(px(8.))
                                .bg(color("--background-surface-higher"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .p(px(8.))
                                .flex()
                                .flex_col()
                                .child(sub_row(
                                    "folder-menu-first",
                                    dc_intl::tr("First in List"),
                                    first_disabled,
                                    dc_state::folders::Placement::First,
                                ))
                                .child(sub_row(
                                    "folder-menu-last",
                                    dc_intl::tr("Last in List"),
                                    last_disabled,
                                    dc_state::folders::Placement::Last,
                                )),
                        )
                    })
                    .into_any_element(),
            );
            rows.push(separator().into_any_element());
        }
        rows.push(
            row(
                "folder-menu-settings",
                dc_intl::tr("Folder Settings"),
                false,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.rail_folders.menu = None;
                this.open_folder_settings(folder, window, cx);
            }))
            .into_any_element(),
        );
        if any_open {
            rows.push(
                row(
                    "folder-menu-collapse",
                    dc_intl::tr("Close All Folders"),
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.rail_folders.menu = None;
                    this.collapse_all_folders(cx);
                }))
                .into_any_element(),
            );
        }
        let frame = div()
            .id("folder-menu")
            .occlude()
            .rounded(px(8.))
            .bg(color("--background-surface-higher"))
            .border_1()
            .border_color(color("--border-subtle"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .p(px(8.))
            .flex()
            .flex_col()
            .children(rows);
        Some(
            div()
                .child(self.menu_backdrop(cx))
                .child(
                    deferred(
                        anchored()
                            .anchor(gpui::Anchor::TopLeft)
                            .position(open.at)
                            .snap_to_window_with_margin(px(8.))
                            .child(super::keyboard::outside(
                                frame,
                                super::keyboard::Layer::FolderMenu,
                                cx,
                            )),
                    )
                    .with_priority(2),
                )
                .into_any_element(),
        )
    }

    pub(super) fn open_folder_settings(
        &mut self,
        folder: u64,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(current) = self.stores.folders.folder(folder).cloned() else {
            return;
        };
        let name = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            editor.set_max_utf16(NAME_MAX);
            editor.set_placeholder_message("Server Folder");
            editor.set_text(current.name.as_deref().unwrap_or(""));
            editor
        });
        let events = cx.subscribe(&name, |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(_) => this.finish_folder_settings(cx),
                EditorEvent::Cancel => this.rail_folders.settings = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        self.rail_folders.settings = Some(FolderSettings {
            folder,
            name,
            color: current.color,
            picker: None,
            _events: events,
            plugin: None,
        });
        cx.notify();
    }

    /// Not in the pinned client: PinDMs' category dialog (`NewCategoryModal`, `plugin_hooks/pin_dms.rs`): New
    /// Category with its first conversation, or Edit Category, in this dialog's layout (Name, Color without the
    /// default swatch, Create or Save).
    pub(in crate::ui) fn open_pin_category_dialog(
        &mut self,
        category: Option<super::plugin_hooks::PinCategory>,
        channel: Option<dc_state::Id>,
        cx: &mut Context<Self>,
    ) {
        let text = category.as_ref().map_or_else(
            || self.pin_category_default_name(),
            |category| category.name.clone(),
        );
        let name = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            editor.set_text(&text);
            editor
        });
        let events = cx.subscribe(&name, |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(_) => this.finish_folder_settings(cx),
                EditorEvent::Cancel => this.rail_folders.settings = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        self.rail_folders.settings = Some(FolderSettings {
            folder: 0,
            name,
            color: Some(u64::from(
                category
                    .as_ref()
                    .map_or(super::plugin_hooks::PIN_DEFAULT_COLOR, |category| {
                        category.color
                    }),
            )),
            picker: None,
            _events: events,
            plugin: Some(super::plugin_hooks::PinCategoryDraft {
                editing: category.map(|category| category.id),
                channel,
            }),
        });
        cx.notify();
    }

    /// Done: the name and colour go into the tree and the tree into the settings.
    fn finish_folder_settings(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.rail_folders.settings.take() else {
            return;
        };
        let name = open.name.read(cx).text().trim().to_owned();
        // Not in the pinned client: PinDMs' category (`plugin_hooks/pin_dms.rs`).
        if let Some(draft) = &open.plugin {
            let color = open
                .color
                .map_or(super::plugin_hooks::PIN_DEFAULT_COLOR, |value| value as u32);
            if name.is_empty() {
                self.rail_folders.settings = Some(open);
                return;
            }
            self.pin_category_save(draft, name, color, cx);
            return;
        }
        if self
            .stores
            .folders
            .set_folder_look(open.folder, &name, open.color)
        {
            self.commit_rail(cx);
        }
        cx.notify();
    }

    fn folder_settings_set_colour(&mut self, rgb: Option<u32>, sync: bool, cx: &mut Context<Self>) {
        let Some(open) = self.rail_folders.settings.as_mut() else {
            return;
        };
        open.color = rgb.map(u64::from);
        if let (Some(rgb), Some(picker)) = (rgb, open.picker.as_mut()) {
            let (hue, saturation, value) = to_hsv(rgb);
            if saturation > 0. {
                picker.hue = hue;
            }
            picker.saturation = saturation;
            picker.value = value;
            if sync {
                picker
                    .hex
                    .update(cx, |editor, _| editor.set_text(&format!("{rgb:06x}")));
            }
        }
        cx.notify();
    }

    /// A drag in the saturation box or on the hue slider.
    fn folder_settings_pick(
        &mut self,
        hue: Option<f32>,
        sv: Option<(f32, f32)>,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.rail_folders.settings.as_mut() else {
            return;
        };
        let Some(picker) = open.picker.as_mut() else {
            return;
        };
        if let Some(hue) = hue {
            picker.hue = hue;
        }
        if let Some((saturation, value)) = sv {
            picker.saturation = saturation;
            picker.value = value;
        }
        let rgb = from_hsv(picker.hue, picker.saturation, picker.value);
        let hex = picker.hex.clone();
        open.color = Some(u64::from(rgb));
        if hex.read(cx).text().trim_start_matches('#') != format!("{rgb:06x}") {
            hex.update(cx, |editor, _| editor.set_text(&format!("{rgb:06x}")));
        }
        cx.notify();
    }

    fn open_folder_picker(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.rail_folders.settings.as_mut() else {
            return;
        };
        let initial = open.color.map_or(DEFAULT_COLOUR, |value| value as u32);
        let hex = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            editor.set_max_utf16(6);
            editor.set_text(&format!("{initial:06x}"));
            editor
        });
        let events = cx.subscribe(&hex, |this, editor, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited) {
                let text = editor.read(cx).text().trim_start_matches('#').to_owned();
                if text.len() == 6
                    && let Ok(value) = u32::from_str_radix(&text, 16)
                {
                    this.folder_settings_set_colour(Some(value), false, cx);
                }
            }
        });
        let (hue, saturation, value) = to_hsv(initial);
        open.picker = Some(Picker {
            hue,
            saturation,
            value,
            hex,
            boxed: Rc::default(),
            slider: Rc::default(),
            _events: events,
        });
        if open.color.is_none() {
            open.color = Some(u64::from(initial));
        }
        cx.notify();
    }

    /// Escape: the picker first, then the dialog. True when something closed.
    pub(super) fn folder_settings_escape(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(open) = self.rail_folders.settings.as_mut() else {
            return false;
        };
        if open.picker.take().is_none() {
            self.rail_folders.settings = None;
        }
        cx.notify();
        true
    }

    pub(super) fn folder_settings_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.rail_folders.settings.as_ref()?;
        let current = open.color.map(|value| value as u32);
        let swatch = |id: SharedString, rgb: u32, selected: bool| {
            div()
                .id(id)
                .absolute()
                .size(px(20.))
                .rounded(px(8.))
                .cursor_pointer()
                .bg(rgba((rgb << 8) | 0xff))
                .flex()
                .items_center()
                .justify_center()
                .when(selected, |this| {
                    this.child(icon("checkmark_large", 15., "--white"))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.folder_settings_set_colour(Some(rgb), true, cx);
                }))
        };
        let mut control = div().relative().h(px(110.)).w_full();
        // Not in the pinned client: PinDMs' dialog has no default swatch (`renderDefaultButton={() => null}`), so
        // its custom swatch takes the first place.
        let pin = open.plugin.as_ref();
        let custom_left = if pin.is_some() { 0. } else { 80. };
        // `defaultContainer_BorJ2C` and `customContainer_BorJ2C`: two 70x50 swatches, 10px apart.
        if pin.is_none() {
            control = control.child(
                div()
                    .id("folder-colour-default")
                    .absolute()
                    .left(px(0.))
                    .top(px(0.))
                    .w(px(70.))
                    .h(px(50.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .bg(rgba((DEFAULT_COLOUR << 8) | 0xff))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(current.is_none(), |this| {
                        this.child(icon("checkmark_large", 25.83, "--white"))
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.rail_folders.settings.as_mut() {
                            open.color = None;
                            open.picker = None;
                        }
                        cx.notify();
                    })),
            );
        }
        let custom_fill = current.filter(|_| true);
        control = control.child(
            div()
                .id("folder-colour-custom")
                .absolute()
                .left(px(custom_left))
                .top(px(0.))
                .w(px(70.))
                .h(px(50.))
                .rounded(px(8.))
                .cursor_pointer()
                .when_some(custom_fill, |this, rgb| this.bg(rgba((rgb << 8) | 0xff)))
                .when(custom_fill.is_none(), |this| {
                    this.border_1().border_color(color("--border-subtle"))
                })
                .child(div().absolute().left(px(51.)).top(px(5.)).child(icon(
                    "clips_pencil",
                    14.,
                    "--white",
                )))
                .on_click(cx.listener(|this, _, _, cx| {
                    if this
                        .rail_folders
                        .settings
                        .as_ref()
                        .is_some_and(|open| open.picker.is_some())
                    {
                        if let Some(open) = this.rail_folders.settings.as_mut() {
                            open.picker = None;
                        }
                        cx.notify();
                    } else {
                        this.open_folder_picker(cx);
                    }
                })),
        );
        for (row_index, row) in PRESETS.iter().enumerate() {
            for (index, rgb) in row.iter().copied().enumerate() {
                control = control.child(
                    swatch(
                        SharedString::from(format!("folder-colour-{row_index}-{index}")),
                        rgb,
                        current == Some(rgb),
                    )
                    .left(px(index as f32 * 30.))
                    .top(px(60. + row_index as f32 * 30.)),
                );
            }
        }
        let field = div()
            .h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--input-border-default"))
            .bg(gpui::rgba(0x0000001f))
            .pl(px(12.))
            .flex()
            .items_center()
            .child(div().flex_1().min_w(px(0.)).child(open.name.clone()));
        // Not in the pinned client: PinDMs' labels (`plugin_hooks/pin_dms.rs`).
        let editing = pin.is_some_and(|draft| draft.editing.is_some());
        let (title, name_label, color_label, done) = match pin {
            None => (
                dc_intl::tr("Folder Settings"),
                dc_intl::tr("Folder Name"),
                dc_intl::tr("Folder Color"),
                dc_intl::tr("Done"),
            ),
            Some(_) if editing => (
                dc_intl::tr("Edit Category"),
                dc_intl::tr("Name"),
                dc_intl::tr("Color"),
                dc_intl::tr("Save"),
            ),
            Some(_) => (
                dc_intl::tr("New Category"),
                dc_intl::tr("Name"),
                dc_intl::tr("Color"),
                dc_intl::tr("Create"),
            ),
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .child(text_line(name_label, 500., "--text-strong").mb(px(8.)))
                    .child(field),
            )
            .child(
                div()
                    .h(px(140.))
                    .child(text_line(color_label, 500., "--text-strong").mb(px(8.)))
                    .child(control),
            )
            .into_any_element();
        let spec = modal::Spec {
            id: "folder-settings",
            title: title.into(),
            subtitle: None,
            body: Some(body),
            buttons: vec![modal::button(
                "folder-settings-done",
                done,
                modal::Tone::Primary,
                |this, _, cx| this.finish_folder_settings(cx),
            )],
            close: Box::new(|this, _, _| this.rail_folders.settings = None),
        };
        Some(modal::render(spec, cx))
    }

    /// The custom colour popout: 254x264 at the custom swatch's lower left (capture: 505,410).
    pub(super) fn folder_picker_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.rail_folders.settings.as_ref()?.picker.as_ref()?;
        let (hue, saturation, value) = (picker.hue, picker.saturation, picker.value);
        let hex = picker.hex.clone();
        let boxed = picker.boxed.clone();
        let slider = picker.slider.clone();
        let area = boxed.clone();
        let pick = move |position: Point<Pixels>, this: &mut Root, cx: &mut Context<Root>| {
            let rect = area.get();
            if rect.size.width <= px(0.) || rect.size.height <= px(0.) {
                return;
            }
            let s = ((position.x - rect.origin.x) / rect.size.width).clamp(0., 1.);
            let v = 1. - ((position.y - rect.origin.y) / rect.size.height).clamp(0., 1.);
            this.folder_settings_pick(None, Some((s, v)), cx);
        };
        let pick_move = pick.clone();
        let saturation_box = div()
            .id("folder-picker-box")
            .relative()
            .w(px(220.))
            .h(px(150.))
            .rounded(px(3.))
            .overflow_hidden()
            .bg(linear_gradient(
                90.,
                linear_color_stop(rgba(0xffffffff), 0.),
                linear_color_stop(rgba((from_hsv(hue, 1., 1.) << 8) | 0xff), 1.),
            ))
            .child(
                canvas(move |bounds, _, _| boxed.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(div().absolute().size_full().bg(linear_gradient(
                180.,
                linear_color_stop(rgba(0x00000000), 0.),
                linear_color_stop(rgba(0x000000ff), 1.),
            )))
            .child(
                div()
                    .absolute()
                    .left(px(saturation * 220. - 4.))
                    .top(px((1. - value) * 150. - 4.))
                    .size(px(8.))
                    .rounded_full()
                    .border_2()
                    .border_color(rgba(0xffffffff)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    pick(event.position, this, cx)
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    pick_move(event.position, this, cx);
                }
            }));
        let stops: [u32; 7] = [
            0xff0000, 0xffff00, 0x00ff00, 0x00ffff, 0x0000ff, 0xff00ff, 0xff0000,
        ];
        let segments = stops.windows(2).map(|pair| {
            div().flex_1().h_full().bg(linear_gradient(
                90.,
                linear_color_stop(rgba((pair[0] << 8) | 0xff), 0.),
                linear_color_stop(rgba((pair[1] << 8) | 0xff), 1.),
            ))
        });
        let bounds = slider.clone();
        let set_hue = move |position: Point<Pixels>, this: &mut Root, cx: &mut Context<Root>| {
            let area = bounds.get();
            if area.size.width <= px(0.) {
                return;
            }
            let ratio = ((position.x - area.origin.x) / area.size.width).clamp(0., 1.);
            this.folder_settings_pick(Some(ratio * 360.), None, cx);
        };
        let set_hue_move = set_hue.clone();
        let hue_bar = div()
            .id("folder-picker-hue")
            .relative()
            .w(px(220.))
            .h(px(8.))
            .rounded(px(3.))
            .flex()
            .child(
                canvas(move |area, _, _| slider.set(area), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .children(segments)
            // `hue-horizontal` pointer: an 8x16 white pill 3px above the bar.
            .child(
                div()
                    .absolute()
                    .top(px(-3.))
                    .left(px(hue / 360. * 212.))
                    .w(px(8.))
                    .h(px(16.))
                    .rounded(px(3.))
                    .bg(rgba(0xffffffff)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    set_hue(event.position, this, cx)
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    set_hue_move(event.position, this, cx);
                }
            }));
        let card = div()
            .id("folder-picker")
            .occlude()
            .w(px(254.))
            .h(px(264.))
            .p(px(16.))
            .rounded(px(4.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .h(px(174.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(saturation_box)
                    .child(hue_bar),
            )
            .child(
                div()
                    .h(px(40.))
                    .pl(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--input-border-active"))
                    .bg(gpui::rgba(0x0000001f))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .pl(px(4.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-muted"))
                            .child("#"),
                    )
                    .child(div().flex_1().min_w(px(0.)).child(hex)),
            )
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if !this.keys.outside_closed_this_press {
                    this.keys.outside_closed_this_press = true;
                    if let Some(open) = this.rail_folders.settings.as_mut() {
                        open.picker = None;
                    }
                    if event.button != gpui::MouseButton::Right {
                        cx.stop_propagation();
                    }
                    cx.notify();
                }
            }));
        Some(
            deferred(
                anchored()
                    .anchor(gpui::Anchor::TopLeft)
                    .position(gpui::point(px(505.), px(410.)))
                    .child(card),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}
