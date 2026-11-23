//! Server Settings > Integrations > Webhooks (module 986057 `eX`, requests module 824953): the breadcrumb, New Webhook,
//! webhooks grouped under "Posting to #channel", each row opening its editor: avatar, Name, Channel, Copy Webhook URL
//! and Delete Webhook behind a confirmation. Edits save through the notice (`PATCH /webhooks/{id}`).
//! Measured in `design/dom/settings/guild-webhooks-darker-cozy.json` and `guild-webhook-open-darker-cozy.json`.

use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::{ChannelKind, Id};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, PathPromptOptions, SharedString,
    Subscription, deferred, div, img, prelude::*, px,
};
use serde_json::Value;

pub(super) const WEBHOOK_SAVED_PREFIX: &str = "guild-webhooks:saved:";
/// `imageUploaderIcon_zIVMHF`'s picture.
const AVATAR_BADGE_ART: &str = "https://discordapp.com/assets/8f18dfd4baacebf4.svg";

/// The webhook open in its editor.
pub(crate) struct OpenWebhook {
    pub(super) id: String,
    pub(super) name: Entity<Editor>,
    channel: String,
    /// A picked image as a data URI, waiting for Save.
    avatar: Option<String>,
    select_open: bool,
    copied: bool,
    _events: Subscription,
}

#[derive(Default)]
pub(crate) struct WebhooksState {
    /// The Webhooks sub-page is showing instead of the Integrations overview.
    pub(super) showing: bool,
    pub(super) open: Option<OpenWebhook>,
    confirm_delete: Option<Value>,
}

/// A webhook's picture: its avatar, or the default one its id selects.
fn avatar_url(webhook: &Value) -> String {
    let id = webhook["id"].as_str().unwrap_or_default();
    match webhook["avatar"].as_str() {
        Some(hash) => format!("https://cdn.discordapp.com/avatars/{id}/{hash}.png?size=128"),
        None => {
            let index = id.parse::<u64>().map_or(0, |id| (id >> 22) % 6);
            format!("https://cdn.discordapp.com/embed/avatars/{index}.png")
        }
    }
}

/// "Nov 8, 2026" (`timestamp` medium) from a snowflake.
fn created_on(id: &str) -> String {
    id.parse::<u64>()
        .ok()
        .and_then(|id| {
            chrono::DateTime::from_timestamp_millis((id >> 22) as i64 + 1_420_070_400_000)
        })
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%b %-d, %Y")
                .to_string()
        })
        .unwrap_or_default()
}

impl Root {
    /// Escape: the Delete Webhook confirmation, then the channel list of an open webhook, close before the page.
    pub(super) fn close_webhook_layer(&mut self) -> bool {
        let Some(open) = self.server_settings.as_mut() else {
            return false;
        };
        if open.webhooks.confirm_delete.take().is_some() {
            return true;
        }
        open.webhooks
            .open
            .as_mut()
            .is_some_and(|hook| std::mem::take(&mut hook.select_open))
    }

    fn guild_webhooks(&self, guild: Id) -> Vec<Value> {
        self.stores
            .integrations
            .webhooks_for_guild(guild)
            .iter()
            .filter(|hook| hook["type"].as_u64() == Some(1))
            .cloned()
            .collect()
    }

    pub(super) fn request_webhook_images(&mut self, guild: Id) {
        self.request_image(AVATAR_BADGE_ART.to_owned(), AVATAR_BADGE_ART.to_owned());
        for hook in self.guild_webhooks(guild) {
            let url = avatar_url(&hook);
            self.request_image(url.clone(), url);
        }
    }

    pub(super) fn show_webhooks(&mut self, guild: Id, cx: &mut Context<Self>) {
        self.request_webhook_images(guild);
        if let Some(open) = self.server_settings.as_mut() {
            open.webhooks.showing = true;
        }
        cx.notify();
    }

    /// Opens a row, or closes the open one.
    fn toggle_webhook(&mut self, webhook: &Value, cx: &mut Context<Self>) {
        let id = webhook["id"].as_str().unwrap_or_default().to_owned();
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        if open
            .webhooks
            .open
            .as_ref()
            .is_some_and(|known| known.id == id)
        {
            open.webhooks.open = None;
            cx.notify();
            return;
        }
        let name = webhook["name"].as_str().unwrap_or_default().to_owned();
        let field = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_text(&name);
            editor
        });
        let events = cx.subscribe(&field, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                }
                EditorEvent::Cancel => this.server_settings = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        open.webhooks.open = Some(OpenWebhook {
            id,
            name: field,
            channel: webhook["channel_id"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            avatar: None,
            select_open: false,
            copied: false,
            _events: events,
        });
        cx.notify();
    }

    fn pick_webhook_avatar(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(uri) = paths
                    .first()
                    .and_then(|path| super::server_settings::data_uri(path))
            {
                let _ = this.update(cx, |this, cx| {
                    if let Some(open) = this
                        .server_settings
                        .as_mut()
                        .and_then(|open| open.webhooks.open.as_mut())
                    {
                        open.avatar = Some(uri);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn webhook_changes(&self, guild: Id, cx: &Context<Self>) -> Option<(String, Value)> {
        let open = self.server_settings.as_ref()?.webhooks.open.as_ref()?;
        let saved = self
            .guild_webhooks(guild)
            .into_iter()
            .find(|hook| hook["id"].as_str() == Some(open.id.as_str()))?;
        let name = open.name.read(cx).text().to_owned();
        let mut body = serde_json::Map::new();
        if saved["name"].as_str() != Some(name.as_str()) {
            body.insert("name".into(), Value::String(name));
        }
        if saved["channel_id"].as_str() != Some(open.channel.as_str()) {
            body.insert("channel_id".into(), Value::String(open.channel.clone()));
        }
        if let Some(avatar) = &open.avatar {
            body.insert("avatar".into(), Value::String(avatar.clone()));
        }
        (!body.is_empty()).then(|| (open.id.clone(), Value::Object(body)))
    }

    pub(super) fn webhook_dirty(&self, guild: Id, cx: &Context<Self>) -> bool {
        self.webhook_changes(guild, cx).is_some()
    }

    pub(super) fn save_webhook(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some((id, body)) = self.webhook_changes(guild, cx) else {
            return;
        };
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{WEBHOOK_SAVED_PREFIX}{guild}")),
            method: reqwest::Method::PATCH,
            route: format!("/webhooks/{id}"),
            body: Some(body.clone()),
        });
        // The list shows the change at once (`WEBHOOK_UPDATE`); the reply refetches it.
        if let Some(mut known) = self
            .stores
            .integrations
            .webhooks_for_guild(guild)
            .iter()
            .find(|known| known["id"].as_str() == Some(id.as_str()))
            .cloned()
        {
            for key in ["name", "channel_id"] {
                if body[key].is_string() {
                    known[key] = body[key].clone();
                }
            }
            self.stores.integrations.webhook_put(guild, &known);
        }
        if let Some(open) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.webhooks.open.as_mut())
        {
            open.avatar = None;
        }
        cx.notify();
    }

    pub(super) fn reset_webhook(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some(id) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.webhooks.open.as_ref())
            .map(|open| open.id.clone())
        else {
            return;
        };
        let Some(saved) = self
            .guild_webhooks(guild)
            .into_iter()
            .find(|hook| hook["id"].as_str() == Some(id.as_str()))
        else {
            return;
        };
        if let Some(open) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.webhooks.open.as_mut())
        {
            let name = saved["name"].as_str().unwrap_or_default().to_owned();
            open.name.update(cx, |editor, _| editor.set_text(&name));
            open.channel = saved["channel_id"].as_str().unwrap_or_default().to_owned();
            open.avatar = None;
        }
        cx.notify();
    }

    fn webhook_small_button(
        id: SharedString,
        label: &'static str,
        critical: bool,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(32.))
            .px(px(11.))
            .rounded(px(8.))
            .border_1()
            .border_color(if critical {
                color("--control-critical-secondary-border-default")
            } else {
                color("--control-secondary-border-default")
            })
            .bg(if critical {
                color("--control-critical-secondary-background-default")
            } else {
                color("--control-secondary-background-default")
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(if critical {
                color("--control-critical-secondary-text-default")
            } else {
                color("--control-secondary-text-default")
            })
            .child(label)
    }

    /// `eX`: the row, and its editor when open.
    fn webhook_row(&self, guild: Id, webhook: &Value, cx: &mut Context<Self>) -> AnyElement {
        let id = webhook["id"].as_str().unwrap_or_default().to_owned();
        let picture = self.images.get(&avatar_url(webhook)).cloned();
        let open = self
            .server_settings
            .as_ref()
            .and_then(|open| open.webhooks.open.as_ref())
            .filter(|open| open.id == id);
        let creator = webhook["user"]["global_name"]
            .as_str()
            .or_else(|| webhook["user"]["username"].as_str())
            .unwrap_or_default()
            .to_owned();
        let name = webhook["name"].as_str().unwrap_or_default().to_owned();
        let toggled = webhook.clone();
        let is_open = open.is_some();
        let header = div()
            .id(SharedString::from(format!("server-webhook-{id}")))
            .h(px(88.))
            .px(px(16.))
            .cursor_pointer()
            .flex()
            .items_center()
            .child(
                div()
                    .size(px(48.))
                    .flex_none()
                    .rounded_full()
                    .overflow_hidden()
                    .children(
                        picture
                            .clone()
                            .map(|picture| img(picture).size(px(48.)).rounded_full()),
                    ),
            )
            .child(
                div()
                    .ml(px(16.))
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(name),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon("webhook_clock", 16., "--interactive-text-default"))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-default"))
                                    .child(format!("Created on {} by {creator}", created_on(&id))),
                            ),
                    ),
            )
            .child(if is_open {
                icon("combo_chevron", 24., "--interactive-text-default").into_any_element()
            } else {
                icon("roles_arrow", 24., "--interactive-text-default").into_any_element()
            })
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_webhook(&toggled, cx)));
        let editor = open.map(|open| {
            let channel_name = self
                .stores
                .guilds
                .iter()
                .find(|known| known.id == guild)
                .and_then(|known| {
                    known
                        .channels
                        .iter()
                        .find(|channel| channel.id.to_string() == open.channel)
                })
                .map_or(String::new(), |channel| format!("#{}", channel.name));
            let token = webhook["token"].as_str().unwrap_or_default().to_owned();
            let link = format!("https://discord.com/api/webhooks/{id}/{token}");
            let delete = webhook.clone();
            let copied = open.copied;
            div()
                .px(px(16.))
                .pb(px(24.))
                .child(div().mb(px(24.)).h(px(1.)).bg(color("--border-subtle")))
                .child(
                    div()
                        .flex()
                        .child(
                            div()
                                .w(px(112.))
                                .pr(px(12.))
                                .flex_none()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .id("server-webhook-avatar")
                                        .relative()
                                        .size(px(100.))
                                        .rounded(px(12.))
                                        .cursor_pointer()
                                        .children(picture.map(|picture| {
                                            img(picture).size(px(100.)).rounded(px(12.))
                                        }))
                                        .child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .right_0()
                                                .size(px(28.))
                                                .rounded_full()
                                                .bg(gpui::white())
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .children(
                                                    self.images
                                                        .get(AVATAR_BADGE_ART)
                                                        .cloned()
                                                        .map(|art| img(art).size(px(18.))),
                                                ),
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.pick_webhook_avatar(cx)
                                        })),
                                )
                                .child(
                                    div()
                                        .mt(px(12.))
                                        .w(px(100.))
                                        .flex()
                                        .text_size(px(10.))
                                        .line_height(px(10.))
                                        .text_color(color("--text-muted"))
                                        .child("Minimum Size:\u{a0}")
                                        .child(
                                            div().font_weight(FontWeight(700.)).child("128x128"),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .ml(px(20.))
                                .flex_1()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(20.))
                                        .child(
                                            div().flex_1().child(Self::field_label("Name")).child(
                                                div()
                                                    .h(px(40.))
                                                    .px(px(12.))
                                                    .rounded(px(8.))
                                                    .border_1()
                                                    .border_color(color("--input-border-default"))
                                                    .bg(color("--input-background-default"))
                                                    .flex()
                                                    .items_center()
                                                    .child(div().w_full().child(open.name.clone())),
                                            ),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .child(Self::field_label("Channel"))
                                                .child(
                                                    div()
                                                        .id("server-webhook-channel")
                                                        .relative()
                                                        .child(
                                                            self.anchor_probe(
                                                                "server-webhook-channel",
                                                            ),
                                                        )
                                                        .h(px(40.))
                                                        .pl(px(12.))
                                                        .rounded(px(8.))
                                                        .border_1()
                                                        .border_color(color(
                                                            "--input-border-default",
                                                        ))
                                                        .bg(color("--input-background-default"))
                                                        .cursor_pointer()
                                                        .flex()
                                                        .items_center()
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .text_size(px(16.))
                                                                .line_height(px(20.))
                                                                .text_color(color("--text-default"))
                                                                .child(channel_name),
                                                        )
                                                        .child(
                                                            div()
                                                                .w(px(36.))
                                                                .flex()
                                                                .justify_center()
                                                                .child(icon(
                                                                    "select_chevron",
                                                                    20.,
                                                                    "--interactive-text-active",
                                                                )),
                                                        )
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            if let Some(open) = this
                                                                .server_settings
                                                                .as_mut()
                                                                .and_then(|open| {
                                                                    open.webhooks.open.as_mut()
                                                                })
                                                            {
                                                                open.select_open =
                                                                    !open.select_open;
                                                            }
                                                            cx.notify();
                                                        })),
                                                ),
                                        ),
                                )
                                .child(div().my(px(24.)).h(px(1.)).bg(color("--border-subtle")))
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(16.))
                                        .child(
                                            Self::webhook_small_button(
                                                SharedString::from("server-webhook-copy"),
                                                if copied {
                                                    "Copied!"
                                                } else {
                                                    "Copy Webhook URL"
                                                },
                                                false,
                                            )
                                            .when(token.is_empty(), |this| this.opacity(0.5))
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    cx.write_to_clipboard(
                                                        gpui::ClipboardItem::new_string(
                                                            link.clone(),
                                                        ),
                                                    );
                                                    if let Some(open) =
                                                        this.server_settings.as_mut().and_then(
                                                            |open| open.webhooks.open.as_mut(),
                                                        )
                                                    {
                                                        open.copied = true;
                                                    }
                                                    cx.notify();
                                                }),
                                            ),
                                        )
                                        .child(
                                            Self::webhook_small_button(
                                                SharedString::from("server-webhook-delete"),
                                                "Delete Webhook",
                                                true,
                                            )
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    if let Some(open) =
                                                        this.server_settings.as_mut()
                                                    {
                                                        open.webhooks.confirm_delete =
                                                            Some(delete.clone());
                                                    }
                                                    cx.notify();
                                                }),
                                            ),
                                        ),
                                ),
                        ),
                )
        });
        div()
            .rounded(px(8.))
            .bg(color("--card-background-default"))
            .flex()
            .flex_col()
            .child(header)
            .children(editor)
            .into_any_element()
    }

    /// The sub-page: breadcrumb, intro, New Webhook, then the webhooks by channel.
    pub(super) fn server_webhooks_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let hooks = self.guild_webhooks(guild);
        let channels: Vec<(String, String)> = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == guild)
            .map(|known| {
                known
                    .channels
                    .iter()
                    .map(|channel| (channel.id.to_string(), channel.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let mut groups: Vec<AnyElement> = Vec::new();
        for (id, name) in &channels {
            let inside: Vec<&Value> = hooks
                .iter()
                .filter(|hook| hook["channel_id"].as_str() == Some(id.as_str()))
                .collect();
            if inside.is_empty() {
                continue;
            }
            groups.push(
                div()
                    .mt(px(24.))
                    .mb(px(16.))
                    .flex()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child("Posting to\u{a0}")
                    .child(
                        div()
                            .text_color(color("--text-strong"))
                            .child(format!("#{name}")),
                    )
                    .into_any_element(),
            );
            for hook in inside {
                groups.push(self.webhook_row(guild, hook, cx));
            }
        }
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(20.))
                    .flex()
                    .items_center()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .child(
                        div()
                            .id("server-webhooks-back")
                            .cursor_pointer()
                            .text_color(color("--text-default"))
                            .child("Integrations")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(open) = this.server_settings.as_mut() {
                                    open.webhooks.showing = false;
                                    open.webhooks.open = None;
                                }
                                cx.notify();
                            })),
                    )
                    .child(div().w(px(18.)).flex().justify_center().child(icon("settings_chevron", 12., "--text-muted")))
                    .child(div().text_color(color("--text-strong")).child("Webhooks")),
            )
            .child(
                div().text_size(px(14.)).line_height(px(18.)).text_color(color("--text-default")).child({
                    // `WL0d0e`: two links in the sentence, the help article and the developer docs.
                    let before = "Webhooks are a simple way to post messages from other apps and websites into Discord using internet magic. ";
                    let (learn, middle, build) = ("Learn more", ", or try ", "building one yourself.");
                    let first = before.len()..before.len() + learn.len();
                    let second = first.end + middle.len()..first.end + middle.len() + build.len();
                    let link = gpui::HighlightStyle { color: Some(color("--text-link").into()), ..Default::default() };
                    gpui::InteractiveText::new(
                        "server-webhooks-help",
                        gpui::StyledText::new(format!("{before}{learn}{middle}{build}")).with_highlights([(first.clone(), link), (second.clone(), link)]),
                    )
                    .on_click(vec![first, second], |index, _, cx| {
                        cx.open_url(if index == 0 {
                            "https://support.discord.com/hc/en-us/articles/228383668"
                        } else {
                            "https://discord.com/developers/docs/resources/webhook"
                        });
                    })
                }),
            )
            .child(div().my(px(32.)).h(px(1.)).bg(color("--border-subtle")))
            .child(
                div().flex().child(
                    div()
                        .id("server-webhooks-new")
                        .h(px(32.))
                        .px(px(11.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-primary-border-default"))
                        .bg(color("--control-primary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--white"))
                        .child("New Webhook")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.create_server_webhook(guild);
                            cx.notify();
                        })),
                ),
            )
            .children(groups)
            .into_any_element()
    }

    /// The open webhook's channel list.
    pub(super) fn webhook_channel_select(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let hook = open.webhooks.open.as_ref()?;
        if !hook.select_open {
            return None;
        }
        let field = self
            .tip_anchors
            .borrow()
            .get("server-webhook-channel")
            .copied()?
            .dilate(px(1.));
        let known = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == open.guild)?;
        let rows: Vec<AnyElement> = known
            .channels
            .iter()
            .filter(|channel| matches!(channel.kind, ChannelKind::Text))
            .map(|channel| {
                let id = channel.id.to_string();
                let selected = id == hook.channel;
                div()
                    .id(SharedString::from(format!("server-webhook-channel-{id}")))
                    .flex_none()
                    .h(px(40.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-strong"))
                    .child(format!("#{}", channel.name))
                    .when(selected, |this| {
                        this.child(icon("select_check", 20., "--text-brand"))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(open) = this
                            .server_settings
                            .as_mut()
                            .and_then(|open| open.webhooks.open.as_mut())
                        {
                            open.channel = id.clone();
                            open.select_open = false;
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let menu_h = ((rows.len() as f32) * 40. + 10.).min(210.);
        let space_below = (self.viewport.1 - f32::from(field.bottom()) - 12.).max(0.);
        let space_above = (f32::from(field.top()) - 12.).max(0.);
        let top = if space_below < menu_h && space_above > space_below {
            (field.top() - px(4.) - px(menu_h)).max(px(8.))
        } else {
            field.bottom() + px(4.)
        };
        Some(
            deferred(
                div()
                    .id("server-webhook-channel-list")
                    .occlude()
                    .absolute()
                    .left(field.left())
                    .top(top)
                    .w(field.size.width)
                    .max_h(px(210.))
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .shadow(vec![super::shadow_high()])
                    .overflow_y_scroll()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(cx.listener(
                        move |this, event: &gpui::MouseDownEvent, _, cx| {
                            if !field.contains(&event.position)
                                && let Some(open) = this
                                    .server_settings
                                    .as_mut()
                                    .and_then(|open| open.webhooks.open.as_mut())
                            {
                                open.select_open = false;
                                cx.notify();
                            }
                        },
                    ))
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    pub(super) fn webhook_confirm_open(&self) -> bool {
        self.server_settings
            .as_ref()
            .is_some_and(|open| open.webhooks.confirm_delete.is_some())
    }

    /// "Delete {name}": the name in bold, then `DELETE /webhooks/{id}`.
    pub(super) fn webhook_delete_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let hook = open.webhooks.confirm_delete.as_ref()?;
        let guild = open.guild;
        let name = hook["name"].as_str().unwrap_or_default().to_owned();
        let id = hook["id"].as_str().unwrap_or_default().to_owned();
        let close = |this: &mut Root| {
            if let Some(open) = this.server_settings.as_mut() {
                open.webhooks.confirm_delete = None;
            }
        };
        let (before, after) = (
            "Are you sure you want to delete the ",
            " webhook? This action cannot be undone.",
        );
        let bold = before.len()..before.len() + name.len();
        Some(super::modal::render(
            super::modal::Spec {
                id: "server-webhook-delete-modal",
                title: format!("Delete {name}").into(),
                subtitle: Some(
                    gpui::StyledText::new(format!("{before}{name}{after}"))
                        .with_highlights([(
                            bold,
                            gpui::HighlightStyle {
                                font_weight: Some(FontWeight(600.)),
                                ..Default::default()
                            },
                        )])
                        .into_any_element(),
                ),
                body: None,
                buttons: vec![
                    super::modal::button(
                        "server-webhook-delete-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        move |this, _, _| close(this),
                    ),
                    super::modal::button(
                        "server-webhook-delete-confirm",
                        "Delete",
                        super::modal::Tone::Critical,
                        move |this, _, _| {
                            let _ = this.requests.send(Request::Rest {
                                key: Some(format!("{WEBHOOK_SAVED_PREFIX}{guild}")),
                                method: reqwest::Method::DELETE,
                                route: format!("/webhooks/{id}"),
                                body: None,
                            });
                            this.stores.integrations.webhook_delete(guild, &id);
                            if let Some(open) = this.server_settings.as_mut() {
                                open.webhooks.open = None;
                            }
                            close(this);
                        },
                    ),
                ],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_pictures_and_dates() {
        let plain = serde_json::json!({"id": "1568692564637524109"});
        assert!(avatar_url(&plain).starts_with("https://cdn.discordapp.com/embed/avatars/"));
        let custom = serde_json::json!({"id": "9", "avatar": "abc"});
        assert_eq!(
            avatar_url(&custom),
            "https://cdn.discordapp.com/avatars/9/abc.png?size=128"
        );
        assert!(created_on("1568692564637524109").ends_with("2026"));
    }
}
