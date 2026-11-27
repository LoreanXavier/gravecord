//! MessageClickActions (Vencord `src/plugins/messageClickActions/index.ts`, GPL-3.0-or-later): single, double and
//! triple clicks on a message run an action of the user's choice. Not part of the pinned client.
//!
//! The click logic is the original's, state for state: `onMouseDown` counts the presses anywhere in the window, the
//! message's click (`onMessageClick`, here the row's `on_click`) decides with the click count (`event.detail`), the
//! keys held (`pressedModifiers`, which Backspace and Delete join) and the three timeouts, and a single click that a
//! double click could still follow waits `clickTimeout` first. The actions are the app's own (the message menu's
//! Edit, Reply, Delete, Pin, reactions and thread), run the way the original runs them: Delete and Pin without the
//! confirmation dialog, React toggling the configured emoji.

use super::super::Root;
use super::super::menu::{Action, ReactionOp};
use crate::plugins::State;
use dc_state::Id;
use gpui::{ClickEvent, ClipboardItem, Context, Modifiers, MouseButton, Window};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

const ID: &str = "MessageClickActions";

/// `ADDITIONAL_REACTION_DELAY_MS`: the wait between the configured extra reactions.
const ADDITIONAL_REACTION_DELAY: Duration = Duration::from_millis(300);
/// `MAX_ADDITIONAL_REACT_EMOJIS`.
const MAX_ADDITIONAL_REACT_EMOJIS: usize = 8;
/// The system account behind system DMs (`isSystemDM`: a DM whose recipient has `system: true`). The app's user
/// object does not keep `system`, and this is the one account that has it.
const SYSTEM_USER: Id = 643_945_264_868_098_049;

/// The original's `ClickAction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum ClickAction {
    None,
    Delete,
    CopyLink,
    CopyId,
    CopyContent,
    CopyUserId,
    Edit,
    Reply,
    React,
    OpenThread,
    OpenTab,
    Quote,
    Pin,
}

impl ClickAction {
    pub(super) fn parse(value: &str) -> Self {
        match value {
            "DELETE" => Self::Delete,
            "COPY_LINK" => Self::CopyLink,
            "COPY_ID" => Self::CopyId,
            "COPY_CONTENT" => Self::CopyContent,
            "COPY_USER_ID" => Self::CopyUserId,
            "EDIT" => Self::Edit,
            "REPLY" => Self::Reply,
            "REACT" => Self::React,
            "OPEN_THREAD" => Self::OpenThread,
            "OPEN_TAB" => Self::OpenTab,
            "QUOTE" => Self::Quote,
            "PIN" => Self::Pin,
            _ => Self::None,
        }
    }
}

/// The original's `Modifier`: the key a click wants held. Backspace and Delete are keys like the others there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::ui) enum Key {
    None,
    Shift,
    Ctrl,
    Alt,
    Backspace,
    Delete,
}

impl Key {
    fn parse(value: &str) -> Self {
        match value {
            "SHIFT" => Self::Shift,
            "CTRL" => Self::Ctrl,
            "ALT" => Self::Alt,
            "BACKSPACE" => Self::Backspace,
            "DELETE" => Self::Delete,
            _ => Self::None,
        }
    }
}

/// The plugin's settings, read once per press.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Settings {
    single: ClickAction,
    single_key: Key,
    single_others: ClickAction,
    single_others_key: Key,
    double: ClickAction,
    double_others: ClickAction,
    double_key: Key,
    triple: ClickAction,
    triple_key: Key,
    react_emoji: String,
    add_additional_reacts: bool,
    additional_react_emojis: String,
    disable_in_dms: bool,
    disable_in_system_dms: bool,
    click_timeout: u64,
    double_click_hold_threshold: u64,
    defer_double_click_for_triple: bool,
    selection_hold_timeout: u64,
    quote_with_reply: bool,
    use_selection_for_quote: bool,
}

impl Settings {
    /// The settings while the plugin is on, else `None` (the plugin does nothing at all then).
    pub(super) fn read(state: &State) -> Option<Self> {
        if !state.is_on(ID) {
            return None;
        }
        let text = |key| match state.value(ID, key) {
            Value::String(text) => text,
            _ => String::new(),
        };
        let action = |key| ClickAction::parse(&text(key));
        let key = |key| Key::parse(&text(key));
        let flag = |key| state.value(ID, key) == Value::Bool(true);
        let ms = |key| state.value(ID, key).as_f64().unwrap_or(0.).max(0.) as u64;
        Some(Self {
            single: action("singleClickAction"),
            single_key: key("singleClickModifier"),
            single_others: action("singleClickOthersAction"),
            single_others_key: key("singleClickOthersModifier"),
            double: action("doubleClickAction"),
            double_others: action("doubleClickOthersAction"),
            double_key: key("doubleClickModifier"),
            triple: action("tripleClickAction"),
            triple_key: key("tripleClickModifier"),
            react_emoji: text("reactEmoji"),
            add_additional_reacts: flag("addAdditionalReacts"),
            additional_react_emojis: text("additionalReactEmojis"),
            disable_in_dms: flag("disableInDms"),
            disable_in_system_dms: flag("disableInSystemDms"),
            click_timeout: ms("clickTimeout"),
            double_click_hold_threshold: ms("doubleClickHoldThreshold"),
            defer_double_click_for_triple: flag("deferDoubleClickForTriple"),
            selection_hold_timeout: ms("selectionHoldTimeout"),
            quote_with_reply: flag("quoteWithReply"),
            use_selection_for_quote: flag("useSelectionForQuote"),
        })
    }

    /// `getConfiguredReactionEmojis`: the reaction emoji, then up to eight extra ones, each once.
    fn reaction_emojis(&self) -> Vec<String> {
        let mut configured = vec![normalize_emoji(&self.react_emoji)];
        if self.add_additional_reacts {
            configured.extend(
                self.additional_react_emojis
                    .split(['\n', ','])
                    .map(normalize_emoji)
                    .filter(Option::is_some)
                    .take(MAX_ADDITIONAL_REACT_EMOJIS),
            );
        }
        let mut seen = Vec::new();
        for emoji in configured.into_iter().flatten() {
            if !seen.contains(&emoji) {
                seen.push(emoji);
            }
        }
        seen
    }
}

/// `normalizeEmoji`: a custom emoji (`<:name:id>`, `<a:name:id>`, `:name:id`, `name:id`) becomes `name:id`, anything
/// else is kept as typed.
fn normalize_emoji(emoji: &str) -> Option<String> {
    let trimmed = emoji.trim();
    if trimmed.is_empty() {
        return None;
    }
    let inner = trimmed
        .strip_prefix("<a:")
        .or_else(|| trimmed.strip_prefix("<:"))
        .or_else(|| trimmed.strip_prefix(':'))
        .unwrap_or(trimmed);
    let inner = inner.strip_suffix('>').unwrap_or(inner);
    if let Some((name, id)) = inner.rsplit_once(':') {
        let name = name.split('~').next().unwrap_or(name);
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            && !id.is_empty()
            && id.chars().all(|c| c.is_ascii_digit())
        {
            return Some(format!("{name}:{id}"));
        }
    }
    Some(trimmed.to_owned())
}

/// What a click on a message is, for [`Clicks::message_click`].
#[derive(Clone, Copy, Debug)]
pub(super) struct Press {
    pub own: bool,
    pub dm: bool,
    pub system_dm: bool,
    /// `event.detail`: 1, 2 or 3.
    pub count: usize,
    pub left: bool,
    pub now: u64,
}

/// What to run, and when.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Plan {
    /// Run the action now. `double`: it is a double click's, which also needs the channel writable.
    Run {
        action: ClickAction,
        key: Key,
        double: bool,
    },
    /// The single click waits: a double click may still come (`singleClickTimeout`).
    LaterSingle { delay: u64, token: u64 },
    /// The double click waits for a triple (`deferDoubleClickForTriple`, `doubleClickTimeout`).
    LaterDouble {
        delay: u64,
        token: u64,
        action: ClickAction,
        key: Key,
    },
}

/// The original's module state: `pressedModifiers`, the press counters and the two pending timeouts.
#[derive(Debug, Default)]
pub(super) struct Clicks {
    /// Backspace and Delete while held (the other keys come from the press itself, see [`Clicks::held`]).
    pressed: BTreeSet<Key>,
    mouse_downs: u32,
    last_down: u64,
    double_detected: bool,
    second_down: u64,
    double_fired: bool,
    /// The waiting single click (`singleClickTimeout`), or the double one (`doubleClickTimeout`), by token.
    single_pending: Option<u64>,
    double_pending: Option<u64>,
    /// The settings and action of the waiting single click.
    single_plan: Option<(ClickAction, Key)>,
    tokens: u64,
}

impl Clicks {
    /// `isModifierPressed`, with `live` the Shift, Ctrl and Alt held now.
    fn held(&self, wanted: Key, live: Modifiers) -> bool {
        let mut keys = self.pressed.clone();
        if live.shift {
            keys.insert(Key::Shift);
        }
        if live.control {
            keys.insert(Key::Ctrl);
        }
        if live.alt {
            keys.insert(Key::Alt);
        }
        match wanted {
            Key::None => keys.is_empty(),
            key => keys.contains(&key),
        }
    }

    /// `resetClickState`.
    fn reset(&mut self) {
        self.mouse_downs = 0;
        self.double_detected = false;
        self.second_down = 0;
    }

    fn token(&mut self) -> u64 {
        self.tokens += 1;
        self.tokens
    }

    /// `keydown` / `keyup` for Backspace and Delete.
    pub(super) fn key(&mut self, key: Key, down: bool) {
        if down {
            self.pressed.insert(key);
        } else {
            self.pressed.remove(&key);
        }
    }

    /// `focusChanged`: a window that lost focus forgets the keys.
    pub(super) fn blurred(&mut self) {
        self.pressed.clear();
    }

    /// `onMouseDown`: a second press within `clickTimeout` is a double click and cancels a waiting single click.
    pub(super) fn mouse_down(&mut self, settings: &Settings, now: u64) {
        if self.mouse_downs >= 1 && now.saturating_sub(self.last_down) < settings.click_timeout {
            self.single_pending = None;
            self.double_detected = true;
            self.second_down = now;
        }
        self.mouse_downs += 1;
        self.last_down = now;
    }

    /// `onMessageClick`.
    pub(super) fn message_click(
        &mut self,
        settings: &Settings,
        press: Press,
        live: Modifiers,
    ) -> Option<Plan> {
        if (settings.disable_in_dms && press.dm)
            || (settings.disable_in_system_dms && press.system_dm)
        {
            return None;
        }
        let (single, single_key) = if press.own {
            (settings.single, settings.single_key)
        } else {
            (settings.single_others, settings.single_others_key)
        };
        let double = if press.own {
            settings.double
        } else {
            settings.double_others
        };
        // Held longer than `selectionHoldTimeout` since the press: the user was selecting text.
        if press.now.saturating_sub(self.last_down) > settings.selection_hold_timeout {
            self.pressed.clear();
            self.reset();
            return None;
        }
        self.single_pending = None;
        if press.count == 3 {
            if !settings.defer_double_click_for_triple {
                self.reset();
                return None;
            }
            self.double_pending = None;
            let plan = (self.held(settings.triple_key, live)
                && settings.triple != ClickAction::None)
                .then(|| {
                    self.pressed.clear();
                    Plan::Run {
                        action: settings.triple,
                        key: settings.triple_key,
                        double: false,
                    }
                });
            self.double_fired = false;
            self.reset();
            return plan;
        }
        let can_double = (self.held(settings.double_key, live) || settings.double_key == Key::None)
            && double != ClickAction::None;
        let can_triple = settings.defer_double_click_for_triple
            && self.held(settings.triple_key, live)
            && settings.triple != ClickAction::None;
        let defer = can_double && can_triple && settings.double_key == settings.triple_key;
        if press.count == 2 {
            self.double_fired = true;
            let quick = !self.double_detected
                || press.now.saturating_sub(self.second_down)
                    < settings.double_click_hold_threshold;
            let plan = if defer {
                let token = self.token();
                self.double_pending = Some(token);
                (can_double && quick).then_some(Plan::LaterDouble {
                    delay: settings.click_timeout,
                    token,
                    action: double,
                    key: settings.double_key,
                })
            } else {
                (can_double && quick).then(|| {
                    self.pressed.clear();
                    Plan::Run {
                        action: double,
                        key: settings.double_key,
                        double: true,
                    }
                })
            };
            self.reset();
            return plan;
        }
        if press.count == 1 && press.left {
            self.double_fired = false;
            let can_double_now = double != ClickAction::None
                && (settings.double_key == Key::None || self.held(settings.double_key, live));
            if can_double_now && single_key == Key::None {
                let token = self.token();
                self.single_pending = Some(token);
                self.single_plan = Some((single, single_key));
                return Some(Plan::LaterSingle {
                    delay: settings.click_timeout,
                    token,
                });
            }
            return self.single_now(single, single_key, live);
        }
        None
    }

    /// `executeSingleClick`: no double click came, and the key is held.
    fn single_now(&mut self, action: ClickAction, key: Key, live: Modifiers) -> Option<Plan> {
        let plan = (!self.double_fired
            && !self.double_detected
            && self.held(key, live)
            && action != ClickAction::None)
            .then(|| {
                self.pressed.clear();
                Plan::Run {
                    action,
                    key,
                    double: false,
                }
            });
        self.reset();
        plan
    }

    /// The single click's timeout ran out: runs it if it was not cancelled.
    pub(super) fn single_due(&mut self, token: u64, live: Modifiers) -> Option<Plan> {
        if self.single_pending != Some(token) {
            return None;
        }
        self.single_pending = None;
        let (action, key) = self.single_plan.take()?;
        self.single_now(action, key, live)
    }

    /// The deferred double click's timeout ran out: runs it if no triple click cancelled it.
    pub(super) fn double_due(&mut self, token: u64) -> bool {
        if self.double_pending != Some(token) {
            return false;
        }
        self.double_pending = None;
        self.pressed.clear();
        true
    }
}

thread_local! {
    static CLICKS: RefCell<Clicks> = RefCell::new(Clicks::default());
}

fn now_ms() -> u64 {
    thread_local! {
        static START: Instant = Instant::now();
    }
    START.with(|start| start.elapsed().as_millis() as u64)
}

/// The plugin's settings while it is on and the plugins are not bypassed.
fn current_settings() -> Option<Settings> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(Settings::read)
}

/// The window-wide listeners of the original (`keydown`, `keyup`, `mousedown`, `focusChanged`), on the app's root.
pub(in crate::ui) fn follow<E: gpui::InteractiveElement>(element: E, window_active: bool) -> E {
    if !window_active {
        CLICKS.with(|clicks| clicks.borrow_mut().blurred());
    }
    element
        .capture_key_down(|event: &gpui::KeyDownEvent, _, _| {
            let key = match event.keystroke.key.as_str() {
                "backspace" => Key::Backspace,
                "delete" => Key::Delete,
                _ => return,
            };
            if current_settings().is_some() {
                CLICKS.with(|clicks| clicks.borrow_mut().key(key, true));
            }
        })
        .capture_key_up(|event: &gpui::KeyUpEvent, _, _| {
            let key = match event.keystroke.key.as_str() {
                "backspace" => Key::Backspace,
                "delete" => Key::Delete,
                _ => return,
            };
            CLICKS.with(|clicks| clicks.borrow_mut().key(key, false));
        })
        .capture_any_mouse_down(|_: &gpui::MouseDownEvent, _, _| {
            if let Some(settings) = current_settings() {
                CLICKS.with(|clicks| clicks.borrow_mut().mouse_down(&settings, now_ms()));
            }
        })
}

/// What the run of an action needs to know about the click.
#[derive(Clone, Copy)]
struct ClickRun {
    message: Id,
    key: Key,
    double: bool,
    shift: bool,
}

impl Root {
    /// A click on a message row (`onMessageClick`).
    pub(in crate::ui) fn message_clicked(
        &mut self,
        message: Id,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(settings) = current_settings() else {
            return;
        };
        let ClickEvent::Mouse(click) = event else {
            return;
        };
        let Some(found) = self.found_message(message) else {
            return;
        };
        let own = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|user| user.id == found.author.id);
        let (dm, system_dm) = self.dm_kind();
        let press = Press {
            own,
            dm,
            system_dm,
            // The press carries the count on every platform (the harness's release always says 1).
            count: click.down.click_count,
            left: click.down.button == MouseButton::Left,
            now: now_ms(),
        };
        let shift = click.up.modifiers.shift;
        let live = click.up.modifiers;
        let plan = CLICKS.with(|clicks| clicks.borrow_mut().message_click(&settings, press, live));
        match plan {
            None => {}
            Some(Plan::Run {
                action,
                key,
                double,
            }) => {
                let context = ClickRun {
                    message,
                    key,
                    double,
                    shift,
                };
                self.run_click_action(action, context, &settings, window, cx);
                // `event.preventDefault()`: the click's own effect (a word selected by the double press) gives way.
                cx.stop_propagation();
            }
            Some(Plan::LaterSingle { delay, token }) => {
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(delay))
                        .await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        let live = window.modifiers();
                        let Some(Plan::Run {
                            action,
                            key,
                            double,
                        }) = CLICKS.with(|clicks| clicks.borrow_mut().single_due(token, live))
                        else {
                            return;
                        };
                        if let Some(settings) = current_settings() {
                            let context = ClickRun {
                                message,
                                key,
                                double,
                                shift,
                            };
                            this.run_click_action(action, context, &settings, window, cx);
                        }
                    });
                })
                .detach();
            }
            Some(Plan::LaterDouble {
                delay,
                token,
                action,
                key,
            }) => {
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(delay))
                        .await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        if !CLICKS.with(|clicks| clicks.borrow_mut().double_due(token)) {
                            return;
                        }
                        if let Some(settings) = current_settings() {
                            let context = ClickRun {
                                message,
                                key,
                                double: true,
                                shift,
                            };
                            this.run_click_action(action, context, &settings, window, cx);
                        }
                    });
                })
                .detach();
                cx.stop_propagation();
            }
        }
    }

    fn found_message(&self, message: Id) -> Option<dc_state::Message> {
        self.channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .and_then(|messages| messages.iter().find(|known| known.id == message))
            .cloned()
    }

    /// `channel.isDM()` and `channel.isSystemDM()` of the open channel (a group DM is neither).
    fn dm_kind(&self) -> (bool, bool) {
        let Some(channel) = self.channel else {
            return (false, false);
        };
        if self.guild.is_some() {
            return (false, false);
        }
        let Some(private) = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel)
        else {
            return (false, false);
        };
        let dm = private.recipients.len() == 1 && private.kind == 1;
        (dm, dm && private.recipients.first() == Some(&SYSTEM_USER))
    }

    /// The open guild channel, for its permissions; `None` in DMs (`PermissionStore.can` is false there).
    fn guild_channel(&self) -> Option<dc_state::Channel> {
        let channel = self.channel?;
        self.guild?;
        self.stores
            .guilds
            .iter()
            .flat_map(|guild| guild.channels.iter())
            .find(|known| known.id == channel)
            .cloned()
    }

    /// `PermissionStore.can(permission, channel)`: false in DMs, as the original's checks see them.
    fn can_here(&self, permission: u64) -> bool {
        self.guild_channel()
            .is_some_and(|channel| self.can_in_channel(&channel, permission))
    }

    /// `canSend`: a DM, or a guild channel the account may write in.
    pub(in crate::ui) fn can_send_here(&self) -> bool {
        self.guild.is_none() || self.can_here(dc_state::permissions::SEND_MESSAGES)
    }

    /// `executeAction`.
    fn run_click_action(
        &mut self,
        action: ClickAction,
        context: ClickRun,
        settings: &Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(found) = self.found_message(context.message) else {
            return;
        };
        // `executeDoubleClick`: a double click acts only where the account may write.
        if context.double && !self.can_send_here() {
            return;
        }
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let own = me == Some(found.author.id);
        let message = context.message;
        let link = || {
            let guild = self
                .guild
                .map_or_else(|| "@me".to_owned(), |guild| guild.to_string());
            format!(
                "https://discord.com/channels/{guild}/{}/{message}",
                found.channel_id
            )
        };
        // `canReply`: a replyable type that is not ephemeral.
        let can_reply = super::super::hover_bar::REPLYABLE.contains(&found.kind)
            && found.flags & dc_state::components::FLAG_EPHEMERAL == 0;
        match action {
            ClickAction::None => {}
            ClickAction::Delete => {
                // `canDelete`: your own message, or a channel where you manage messages.
                if own || self.can_here(dc_state::permissions::MANAGE_MESSAGES) {
                    self.run_unconfirmed(Action::Delete, message, cx);
                }
            }
            ClickAction::CopyLink => self.copy_with_toast(link(), "Link copied!", cx),
            ClickAction::CopyId => {
                self.copy_with_toast(message.to_string(), "Message ID copied!", cx)
            }
            ClickAction::CopyContent => {
                self.copy_with_toast(found.content.clone(), "Message content copied!", cx)
            }
            ClickAction::CopyUserId => {
                self.copy_with_toast(found.author.id.to_string(), "User ID copied!", cx)
            }
            ClickAction::Edit => {
                let editing = self
                    .editing
                    .as_ref()
                    .is_some_and(|(target, _)| *target == message);
                if own && !editing && self.send_state_of(message).is_none() {
                    self.run_action(Action::Edit, message, window, cx);
                }
            }
            ClickAction::Reply => {
                if can_reply && self.can_send_here() {
                    // `shouldMention`: Shift turns the mention off, unless Shift was the key the click wanted.
                    let mention = !(context.shift && context.key != Key::Shift);
                    self.begin_reply(found, window, cx);
                    self.reply_ping = mention;
                }
            }
            ClickAction::Quote => {
                if !can_reply {
                    self.put_app_toast("Cannot quote this message type".to_owned(), false, cx);
                    return;
                }
                let mut content = found.content.clone();
                if settings.use_selection_for_quote {
                    let selection = self.selected_text();
                    let selection = selection.trim();
                    if !selection.is_empty() && content.contains(selection) {
                        content = selection.to_owned();
                    }
                }
                if content.is_empty() {
                    return;
                }
                let quote = quote_text(&content);
                self.composer
                    .update(cx, |composer, cx| composer.insert(&quote, cx));
                if settings.quote_with_reply {
                    self.begin_reply(found, window, cx);
                    self.reply_ping = false;
                }
            }
            ClickAction::Pin => {
                if !self.can_here(dc_state::permissions::MANAGE_MESSAGES) {
                    self.put_app_toast("Cannot pin: Missing permissions".to_owned(), false, cx);
                    return;
                }
                self.run_unconfirmed(Action::Pin, message, cx);
            }
            ClickAction::React => self.react_configured(&found, settings, cx),
            ClickAction::OpenThread => self.run_action(Action::Thread, message, window, cx),
            ClickAction::OpenTab => cx.open_url(&link()),
        }
        cx.notify();
    }

    /// `copyWithToast`.
    fn copy_with_toast(&mut self, text: String, toast: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.show_success_toast(toast.to_owned(), cx);
    }

    /// `reactWithConfiguredEmojis`: toggles the reaction emoji, then adds the extra ones 300 ms apart.
    fn react_configured(
        &mut self,
        found: &dc_state::Message,
        settings: &Settings,
        cx: &mut Context<Self>,
    ) {
        let mut emojis = settings.reaction_emojis().into_iter();
        let Some(first) = emojis.next() else {
            return;
        };
        let allowed = self.guild.is_none()
            || self.can_here(
                dc_state::permissions::ADD_REACTIONS | dc_state::permissions::READ_MESSAGE_HISTORY,
            );
        if !allowed {
            self.put_app_toast("Cannot react: Missing permissions".to_owned(), false, cx);
            return;
        }
        let (channel, message) = (found.channel_id, found.id);
        let reacted = found.reactions.iter().any(|reaction| {
            let name = match reaction.emoji_id {
                Some(id) => format!("{}:{id}", reaction.name),
                None => reaction.name.clone(),
            };
            reaction.me && name == first
        });
        self.send_and_apply_reaction(
            channel,
            message,
            ReactionOp {
                emoji: first,
                add: !reacted,
                location: "Message",
                burst: false,
            },
            cx,
        );
        let rest: Vec<String> = emojis.collect();
        if rest.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            for emoji in rest {
                cx.background_executor()
                    .timer(ADDITIONAL_REACTION_DELAY)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.send_and_apply_reaction(
                        channel,
                        message,
                        ReactionOp {
                            emoji,
                            add: true,
                            location: "Message",
                            burst: false,
                        },
                        cx,
                    );
                });
            }
        })
        .detach();
    }
}

/// `quoteMessage`: every line behind `> `, and a line break after.
fn quote_text(content: &str) -> String {
    let mut quote = content
        .split('\n')
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    quote.push('\n');
    quote
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn on() -> State {
        let mut state = State::default();
        state.set_enabled(ID, true);
        state
    }

    fn settings_of(state: &State) -> Settings {
        Settings::read(state).expect("on")
    }

    fn press(count: usize, now: u64) -> Press {
        Press {
            own: true,
            dm: false,
            system_dm: false,
            count,
            left: true,
            now,
        }
    }

    fn none() -> Modifiers {
        Modifiers::default()
    }

    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Modifiers::default()
        }
    }

    /// A press and its click `held` ms later.
    fn click(
        clicks: &mut Clicks,
        settings: &Settings,
        count: usize,
        at: u64,
        live: Modifiers,
    ) -> Option<Plan> {
        clicks.mouse_down(settings, at);
        clicks.message_click(settings, press(count, at + 50), live)
    }

    #[test]
    fn the_defaults_are_the_originals() {
        let settings = settings_of(&on());
        assert_eq!(
            (settings.single, settings.single_key),
            (ClickAction::Delete, Key::Backspace)
        );
        assert_eq!(
            (settings.single_others, settings.single_others_key),
            (ClickAction::Delete, Key::Backspace)
        );
        assert_eq!(
            (settings.double, settings.double_others),
            (ClickAction::Edit, ClickAction::Reply)
        );
        assert_eq!(settings.double_key, Key::None);
        assert_eq!(
            (settings.triple, settings.triple_key),
            (ClickAction::React, Key::None)
        );
        assert_eq!(settings.react_emoji, "\u{1f480}");
        assert!(!settings.add_additional_reacts && settings.additional_react_emojis.is_empty());
        assert!(!settings.disable_in_dms && settings.disable_in_system_dms);
        assert_eq!(
            (
                settings.click_timeout,
                settings.double_click_hold_threshold,
                settings.selection_hold_timeout
            ),
            (300, 150, 300)
        );
        assert!(!settings.defer_double_click_for_triple);
        assert!(settings.quote_with_reply && !settings.use_selection_for_quote);
        assert!(Settings::read(&State::default()).is_some(), "on by default");
        let mut off = on();
        off.set_enabled(ID, false);
        assert!(Settings::read(&off).is_none());
    }

    /// Backspace held + click deletes at once (the single click needs a key, so it does not wait).
    #[test]
    fn backspace_click_deletes() {
        let settings = settings_of(&on());
        let mut clicks = Clicks::default();
        assert_eq!(click(&mut clicks, &settings, 1, 1_000, none()), None);
        clicks.key(Key::Backspace, true);
        assert_eq!(
            click(&mut clicks, &settings, 1, 5_000, none()),
            Some(Plan::Run {
                action: ClickAction::Delete,
                key: Key::Backspace,
                double: false
            })
        );
        // `pressedModifiers.clear()` after the action: the next click needs the key again (its autorepeat).
        assert_eq!(click(&mut clicks, &settings, 1, 9_000, none()), None);
    }

    #[test]
    fn a_double_click_edits_your_message_and_replies_to_others() {
        let settings = settings_of(&on());
        let mut clicks = Clicks::default();
        assert_eq!(click(&mut clicks, &settings, 1, 1_000, none()), None);
        let plan = click(&mut clicks, &settings, 2, 1_100, none());
        assert_eq!(
            plan,
            Some(Plan::Run {
                action: ClickAction::Edit,
                key: Key::None,
                double: true
            })
        );
        let mut others = press(2, 3_150);
        others.own = false;
        clicks.mouse_down(&settings, 3_100);
        assert_eq!(
            clicks.message_click(&settings, others, none()),
            Some(Plan::Run {
                action: ClickAction::Reply,
                key: Key::None,
                double: true
            })
        );
        // With the key "None" a double click acts whatever is held (`|| doubleClickModifier === "NONE"`).
        clicks.mouse_down(&settings, 5_000);
        assert!(matches!(
            clicks.message_click(&settings, press(2, 5_050), shift()),
            Some(Plan::Run {
                action: ClickAction::Edit,
                ..
            })
        ));
    }

    #[test]
    fn the_double_click_key_must_be_held() {
        let mut state = on();
        state.set_value(ID, "doubleClickModifier", &json!("SHIFT"));
        let settings = settings_of(&state);
        let mut clicks = Clicks::default();
        assert_eq!(click(&mut clicks, &settings, 2, 1_000, none()), None);
        assert!(matches!(
            click(&mut clicks, &settings, 2, 2_000, shift()),
            Some(Plan::Run {
                action: ClickAction::Edit,
                key: Key::Shift,
                ..
            })
        ));
    }

    /// A press held past `selectionHoldTimeout` is a selection, and a slow second press past
    /// `doubleClickHoldThreshold` lets the double click select a word instead.
    #[test]
    fn long_presses_select_text_instead() {
        let settings = settings_of(&on());
        let mut clicks = Clicks::default();
        clicks.mouse_down(&settings, 1_000);
        assert_eq!(
            clicks.message_click(&settings, press(2, 1_400), none()),
            None
        );
        // First click (Backspace not held: nothing, and the counters reset), then a second press held 200 ms.
        let mut clicks = Clicks::default();
        clicks.mouse_down(&settings, 2_000);
        clicks.mouse_down(&settings, 2_100);
        assert_eq!(
            clicks.message_click(&settings, press(2, 2_300), none()),
            None
        );
        let mut clicks = Clicks::default();
        clicks.mouse_down(&settings, 2_000);
        clicks.mouse_down(&settings, 2_100);
        assert!(
            clicks
                .message_click(&settings, press(2, 2_200), none())
                .is_some()
        );
    }

    /// With the single action on no key, the click waits for `clickTimeout` and a double click cancels it.
    #[test]
    fn a_keyless_single_click_waits_for_a_double() {
        let mut state = on();
        state.set_value(ID, "singleClickModifier", &json!("NONE"));
        state.set_value(ID, "singleClickAction", &json!("COPY_ID"));
        let settings = settings_of(&state);
        let mut clicks = Clicks::default();
        let Some(Plan::LaterSingle { delay, token }) =
            click(&mut clicks, &settings, 1, 1_000, none())
        else {
            panic!("waits");
        };
        assert_eq!(delay, 300);
        assert!(matches!(
            clicks.single_due(token, none()),
            Some(Plan::Run {
                action: ClickAction::CopyId,
                ..
            })
        ));
        let Some(Plan::LaterSingle { token, .. }) = click(&mut clicks, &settings, 1, 5_000, none())
        else {
            panic!("waits");
        };
        assert!(click(&mut clicks, &settings, 2, 5_100, none()).is_some());
        assert_eq!(
            clicks.single_due(token, none()),
            None,
            "the double click cancelled it"
        );
    }

    /// Triple clicks only act with `deferDoubleClickForTriple`, which then holds the double click back.
    #[test]
    fn triple_clicks_need_the_deferred_double() {
        let settings = settings_of(&on());
        let mut clicks = Clicks::default();
        assert_eq!(click(&mut clicks, &settings, 3, 1_000, none()), None);
        let mut state = on();
        state.set_value(ID, "deferDoubleClickForTriple", &json!(true));
        let settings = settings_of(&state);
        let mut clicks = Clicks::default();
        let Some(Plan::LaterDouble { token, action, .. }) =
            click(&mut clicks, &settings, 2, 1_000, none())
        else {
            panic!("deferred");
        };
        assert_eq!(action, ClickAction::Edit);
        assert_eq!(
            click(&mut clicks, &settings, 3, 1_150, none()),
            Some(Plan::Run {
                action: ClickAction::React,
                key: Key::None,
                double: false
            })
        );
        assert!(
            !clicks.double_due(token),
            "the triple click cancelled the double"
        );
    }

    #[test]
    fn dms_and_system_dms_can_be_left_alone() {
        let mut state = on();
        state.set_value(ID, "disableInDms", &json!(true));
        let settings = settings_of(&state);
        let mut clicks = Clicks::default();
        clicks.mouse_down(&settings, 1_000);
        let mut in_dm = press(2, 1_050);
        in_dm.dm = true;
        assert_eq!(clicks.message_click(&settings, in_dm, none()), None);
        let settings = settings_of(&on());
        in_dm.system_dm = true;
        clicks.mouse_down(&settings, 2_000);
        in_dm.now = 2_050;
        assert_eq!(
            clicks.message_click(&settings, in_dm, none()),
            None,
            "disableInSystemDms is on by default"
        );
        in_dm.system_dm = false;
        clicks.mouse_down(&settings, 3_000);
        in_dm.now = 3_050;
        assert!(clicks.message_click(&settings, in_dm, none()).is_some());
    }

    #[test]
    fn emojis_normalize_as_the_original() {
        assert_eq!(normalize_emoji(" \u{1f480} ").as_deref(), Some("\u{1f480}"));
        assert_eq!(normalize_emoji("<:blob:123>").as_deref(), Some("blob:123"));
        assert_eq!(
            normalize_emoji("<a:party_blob:45>").as_deref(),
            Some("party_blob:45")
        );
        assert_eq!(normalize_emoji(":blob~1:123").as_deref(), Some("blob:123"));
        assert_eq!(normalize_emoji("   "), None);
        let mut state = on();
        state.set_value(ID, "addAdditionalReacts", &json!(true));
        state.set_value(
            ID,
            "additionalReactEmojis",
            &json!("\u{1f44d},\u{1f480}\n<:blob:123>,,"),
        );
        assert_eq!(
            settings_of(&state).reaction_emojis(),
            ["\u{1f480}", "\u{1f44d}", "blob:123"]
        );
    }

    #[test]
    fn quotes_mark_every_line() {
        assert_eq!(quote_text("a\nb"), "> a\n> b\n");
    }

    #[test]
    fn every_original_action_parses() {
        for (text, action) in [
            ("NONE", ClickAction::None),
            ("DELETE", ClickAction::Delete),
            ("COPY_LINK", ClickAction::CopyLink),
            ("COPY_ID", ClickAction::CopyId),
            ("COPY_CONTENT", ClickAction::CopyContent),
            ("COPY_USER_ID", ClickAction::CopyUserId),
            ("EDIT", ClickAction::Edit),
            ("REPLY", ClickAction::Reply),
            ("REACT", ClickAction::React),
            ("OPEN_THREAD", ClickAction::OpenThread),
            ("OPEN_TAB", ClickAction::OpenTab),
            ("QUOTE", ClickAction::Quote),
            ("PIN", ClickAction::Pin),
        ] {
            assert_eq!(ClickAction::parse(text), action);
        }
    }
}
