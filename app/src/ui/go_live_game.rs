//! Game-specific Go Live: module 483364's panel button and 742527's confirmation/channel selector.
use super::{Root, color, editor_style, go_live, icon};
use crate::editor::{Editor, EditorEvent};
use crate::parity::Parity;
use dc_media::screen::{Source, SourceId};
use dc_state::{ChannelKind, Id, permissions};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, ObjectFit, RenderImage,
    Subscription, Window, div, img, prelude::*, px,
};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

pub(super) struct GameShare {
    pub name: String,
    pub application_id: Option<String>,
    pub choose_channel: bool,
    pub source: Option<Source>,
    pub preview: Option<Arc<RenderImage>>,
    pub pending_channel: Option<(Id, Id)>,
    join_attempt: u64,
    pub(super) search: Entity<Editor>,
    query: String,
    _events: Subscription,
}

#[cfg(not(feature = "capture"))]
fn game_source(pid: u32, name: String) -> Option<(Source, Option<dc_media::Picture>)> {
    let window = dc_detect::main_window(pid)?;
    let source = dc_media::screen::sources()
        .ok()?
        .into_iter()
        .find(|source| source.id == SourceId::Window(window))?;
    let preview = dc_media::screen::thumbnail(&source.id, 960, 540).ok();
    Some((Source { name, ..source }, preview))
}

#[cfg(feature = "capture")]
fn game_source(_pid: u32, name: String) -> Option<(Source, Option<dc_media::Picture>)> {
    let (width, height) = (960, 540);
    let mut bytes = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..height {
        for x in 0..width {
            bytes.extend_from_slice(match x / 320 {
                0 => &[192, 64, 64, 255],
                1 => &[64, 192, 64, 255],
                _ => &[64, 64, 192, 255],
            });
        }
    }
    Some((
        Source {
            id: SourceId::Window(1),
            name,
            width: 1280,
            height: 720,
            refresh_hz: 60,
            icon: None,
        },
        dc_media::Picture::rgba(width, height, bytes),
    ))
}

impl Root {
    fn game_stream_destination(&self, guild: Id, channel: Id, require_connect: bool) -> bool {
        if guild == 0 {
            return self
                .stores
                .private_channels
                .iter()
                .any(|known| known.id == channel);
        }
        let Some(guild) = self.stores.guild(guild) else {
            return false;
        };
        let Some(channel) = guild.channels.iter().find(|known| known.id == channel) else {
            return false;
        };
        let Some(me) = self.stores.me() else {
            return false;
        };
        permissions::can_stream_in_channel(
            guild,
            channel,
            me,
            self.stores.afk_channels.get(&guild.id).copied(),
            require_connect,
        )
    }

    pub(super) fn game_stream_allowed(&self) -> bool {
        self.call
            .as_ref()
            .is_none_or(|call| self.game_stream_destination(call.guild, call.channel, false))
    }

    pub(super) fn open_game_share(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game) = self.panel_running_game() else {
            return;
        };
        let Some(pid) = game.pid else {
            return;
        };
        if !self.game_stream_allowed() {
            return;
        }
        let choose_channel = self.call.is_none();
        let search = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            editor.set_placeholder_message(dc_intl::t!("3jvv+6"));
            editor
        });
        let events = cx.subscribe(&search, |root, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {
                    if let Some(game) = root.go_live.as_mut().and_then(|state| state.game.as_mut())
                    {
                        game.query = editor.read(cx).text().to_owned();
                    }
                }
                EditorEvent::Cancel => root.close_go_live(cx),
                EditorEvent::Submit(_) => {
                    if let Some((guild, channel)) = root.game_share_channels().first().copied() {
                        root.game_share_choose_channel(guild, channel, cx);
                    }
                }
            }
            cx.notify();
        });
        if choose_channel {
            window.focus(&search.read(cx).focus_handle(cx), cx);
        }
        self.go_live_generation += 1;
        let generation = self.go_live_generation;
        let mut state = go_live::State::new(generation, self.stream_account());
        let name = game.name.unwrap_or_else(|| dc_intl::tr("Unknown").into());
        state.game = Some(GameShare {
            name: name.clone(),
            application_id: game.id,
            choose_channel,
            source: None,
            preview: None,
            pending_channel: None,
            join_attempt: 0,
            search,
            query: String::new(),
            _events: events,
        });
        self.go_live = Some(state);
        cx.spawn(async move |root, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { game_source(pid, name) })
                .await;
            let _ = root.update(cx, |root, cx| {
                let Some(state) = root
                    .go_live
                    .as_mut()
                    .filter(|state| state.generation == generation)
                else {
                    return;
                };
                state.fetching = false;
                if let Some((source, preview)) = result {
                    let game = state.game.as_mut().unwrap();
                    game.source = Some(source);
                    game.preview = preview.as_ref().and_then(super::video_tiles::render_image);
                }
                root.game_share_pending(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn game_share_channels(&self) -> Vec<(Id, Id)> {
        let Some(game) = self.go_live.as_ref().and_then(|state| state.game.as_ref()) else {
            return Vec::new();
        };
        if !game.query.trim().is_empty() {
            return self
                .compute_switcher_rows(&format!("!{}", game.query))
                .into_iter()
                .filter_map(|row| {
                    if let super::quickswitcher::SwitcherRow::Result {
                        destination: super::quickswitcher::Destination::VoiceChannel { guild, id },
                        ..
                    } = row
                    {
                        self.game_stream_destination(guild, id, true)
                            .then_some((guild, id))
                    } else {
                        None
                    }
                })
                .collect();
        }
        let (_, frequent) = self.switcher_frecency();
        let recent: Vec<Id> = self
            .channel_history
            .iter()
            .chain(frequent.iter())
            .copied()
            .collect();
        let ranked: HashSet<Id> = recent.iter().copied().collect();
        let mut candidates = self
            .current_guild()
            .map(|guild| {
                let mut channels: Vec<_> = guild
                    .channels
                    .iter()
                    .filter(|channel| self.game_stream_destination(guild.id, channel.id, true))
                    .map(|channel| (guild.id, channel.id))
                    .collect();
                channels.sort_by_key(|(_, id)| !ranked.contains(id));
                channels
            })
            .unwrap_or_default();
        let mut seen: HashSet<Id> = candidates.iter().map(|(_, id)| *id).collect();
        for id in recent {
            if seen.contains(&id) {
                continue;
            }
            if let Some(guild) = self.stores.guilds.iter().find(|guild| {
                guild
                    .channels
                    .iter()
                    .any(|channel| channel.id == id && channel.kind == ChannelKind::Voice)
            }) && self.game_stream_destination(guild.id, id, true)
            {
                seen.insert(id);
                candidates.push((guild.id, id));
            }
        }
        candidates
    }

    fn game_share_choose_channel(&mut self, guild: Id, channel: Id, cx: &mut Context<Self>) {
        if !self.game_stream_destination(guild, channel, true) {
            return;
        }
        self.game_share_wait(guild, channel, cx);
        self.request_join_voice(guild, channel, cx);
        self.game_share_pending(cx);
    }

    fn game_share_wait(&mut self, guild: Id, channel: Id, cx: &mut Context<Self>) {
        let Some(state) = self.go_live.as_mut() else {
            return;
        };
        let Some(game) = state.game.as_mut() else {
            return;
        };
        game.pending_channel = Some((guild, channel));
        game.join_attempt += 1;
        let attempt = game.join_attempt;
        let generation = state.generation;
        // Module 90804 resolves a failed join after ten seconds, leaving the chooser open.
        cx.spawn(async move |root, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(10))
                .await;
            let _ = root.update(cx, |root, cx| {
                if let Some(game) = root
                    .go_live
                    .as_mut()
                    .filter(|state| state.generation == generation)
                    .and_then(|state| state.game.as_mut())
                    && game.pending_channel == Some((guild, channel))
                    && game.join_attempt == attempt
                {
                    game.pending_channel = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn game_share_start(&mut self, cx: &mut Context<Self>) {
        if let Some(call) = &self.call {
            self.game_share_wait(call.guild, call.channel, cx);
            self.game_share_pending(cx);
        }
    }

    pub(super) fn game_share_pending(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.go_live.as_ref() else {
            return;
        };
        let Some(game) = state.game.as_ref() else {
            return;
        };
        let Some((guild, channel)) = game.pending_channel else {
            return;
        };
        if !self
            .call
            .as_ref()
            .is_some_and(|call| call.connected && call.guild == guild && call.channel == channel)
            || self.stream_identity().is_none()
            || !self.game_stream_destination(guild, channel, false)
        {
            return;
        }
        let Some(source) = game.source.clone() else {
            return;
        };
        let prefs = state.prefs;
        let (preset, resolution, fps) = prefs.start_choice(self.stream_account());
        let Some((settings, resolution_used, fps_used)) =
            go_live::settings_for(&source, resolution, fps, prefs.sound)
        else {
            return;
        };
        go_live::Prefs {
            preset,
            resolution,
            fps,
            ..prefs
        }
        .save();
        let picture = source
            .icon
            .as_ref()
            .and_then(super::video_tiles::render_image);
        let started = super::voice_video::Started {
            name: game.name.clone(),
            info: source,
            quality: (resolution_used, fps_used),
            icon: picture,
        };
        self.close_go_live(cx);
        self.stream_restart(
            dc_voice::StreamSource::Screen(settings),
            started,
            self.vv.focus.tile,
            cx,
        );
    }

    pub(super) fn game_share_body(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let game = self.go_live.as_ref()?.game.as_ref()?;
        if !game.choose_channel {
            return Some(
                div()
                    .parity("golive-game-preview")
                    .size_full()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when_some(game.preview.clone(), |element, preview| {
                        element.child(
                            img(preview)
                                .w_full()
                                .aspect_ratio(16. / 9.)
                                .object_fit(ObjectFit::Contain),
                        )
                    })
                    .when(game.preview.is_none(), |element| {
                        element.child(super::login::wandering_cubes())
                    })
                    .into_any_element(),
            );
        }
        let focused = game.search.read(cx).focus_handle(cx).is_focused(window);
        let search = div()
            .id("golive-game-search")
            .parity_with_border("golive-game-search", if focused { 2. } else { 1. })
            .h(px(40.))
            .flex_none()
            .rounded(px(8.))
            .border(px(if focused { 2. } else { 1. }))
            .border_color(color(if focused {
                "--input-border-active"
            } else {
                "--input-border-default"
            }))
            .bg(color("--input-background-default"))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(icon("picker_search", 16., "--input-text-default"))
            .child(div().flex_1().min_w(px(0.)).child(game.search.clone()))
            .when(!game.query.is_empty(), |element| {
                element.child(
                    div()
                        .id("golive-game-search-clear")
                        .cursor_pointer()
                        .size(px(30.))
                        .mr(px(-7.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("discover_clear", 16., "--text-muted"))
                        .on_click(cx.listener(|root, _, window, cx| {
                            if let Some(game) =
                                root.go_live.as_mut().and_then(|state| state.game.as_mut())
                            {
                                game.query.clear();
                                game.search.update(cx, |editor, cx| {
                                    editor.set_text("");
                                    cx.notify();
                                });
                                window.focus(&game.search.read(cx).focus_handle(cx), cx);
                            }
                            cx.notify();
                        })),
                )
            });
        let channels = self.game_share_channels();
        let first_channel = channels.first().copied();
        let body = if channels.is_empty() {
            div().flex_1().flex().flex_col().items_center().justify_center()
                .child(div().mb(px(16.)).child(icon("picker_search", 24., "--text-muted")))
                .child(div().mb(px(8.)).text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight(600.))
                    .text_color(color("--text-strong")).child(dc_intl::tr("There are no voice channels available")))
                .child(div().text_size(px(14.)).line_height(px(18.)).text_color(color("--text-muted"))
                    .child(dc_intl::tr("Try entering a different keyword to find a voice channel to stream in.")))
                .into_any_element()
        } else {
            div()
                .id("golive-game-channels")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .pr(px(8.))
                .child(
                    div()
                        .h(px(16.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::tr("Recent Channels")),
                )
                .children(channels.into_iter().filter_map(|(guild_id, channel_id)| {
                    let guild = self.stores.guild(guild_id)?;
                    let channel = guild
                        .channels
                        .iter()
                        .find(|channel| channel.id == channel_id)?;
                    let picture = self.images.get(&format!("guild:{guild_id}")).cloned();
                    let emblem = div()
                        .size(px(40.))
                        .rounded(px(12.))
                        .flex_none()
                        .overflow_hidden()
                        .bg(color("--background-base-lowest"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when_some(picture.clone(), |element, picture| {
                            element.child(img(picture).size_full())
                        })
                        .when(picture.is_none(), |element| element.child(guild.acronym()));
                    let row = div()
                        .id(gpui::SharedString::from(format!(
                            "golive-game-channel-{channel_id}"
                        )))
                        .when(first_channel == Some((guild_id, channel_id)), |row| {
                            row.parity("golive-game-first-channel")
                        })
                        .h(px(56.))
                        .px(px(8.))
                        .py(px(8.))
                        .rounded(px(12.))
                        .cursor_pointer()
                        .hover(|style| style.bg(color("--background-mod-strong")))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(emblem)
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--interactive-text-default"))
                                        .child(icon(
                                            "voice_channel",
                                            16.,
                                            "--interactive-text-default",
                                        ))
                                        .child(channel.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(guild.name.clone()),
                                ),
                        )
                        .on_click(cx.listener(move |root, _, _, cx| {
                            root.game_share_choose_channel(guild_id, channel_id, cx)
                        }));
                    let handle = self.stop(&format!("golive-game-channel-{channel_id}"), cx);
                    Some(
                        super::keyboard::focusable(row, &handle, true, self.keyboard_mode())
                            .into_any_element(),
                    )
                }))
                .into_any_element()
        };
        Some(
            div()
                .parity("golive-game-channel-selector")
                .size_full()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(search)
                .child(body)
                .into_any_element(),
        )
    }
}
