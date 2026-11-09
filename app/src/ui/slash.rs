//! Slash commands and the app launcher. The `/` list is fed by the application command index
//! (`/guilds/{id}/application-command-index`, `/channels/{id}/application-command-index`) and the built-in commands of
//! modules 12115 and 743361; a chosen command becomes chips in the composer and its options are filled one after the
//! other. App commands are sent as `POST /interactions` (type 2); built-ins run their own effect, as the official
//! `execute` functions do. Strings are from `design/snapshot/dumps/STRINGS/en-US.json`; rows and sizes from
//! `design/dom/channel/autocomplete-slash-darker-cozy.json` and `apps-launcher-darker-cozy.json`.

use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::{Channel, ChannelKind, Id, permissions};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, MouseButton, ScrollHandle,
    Window, div, img, prelude::*, px, relative,
};
use serde_json::{Value, json};

/// `ApplicationCommandOptionType`.
const SUB_COMMAND: u64 = 1;
const SUB_COMMAND_GROUP: u64 = 2;
const STRING: u64 = 3;
const INTEGER: u64 = 4;
const BOOLEAN: u64 = 5;
const USER: u64 = 6;
const CHANNEL: u64 = 7;
const ROLE: u64 = 8;
const MENTIONABLE: u64 = 9;
const NUMBER: u64 = 10;
const ATTACHMENT: u64 = 11;

/// The id the official gives the built-in "application".
const BUILT_IN_APP: &str = "-1";
/// Rows the list shows before it scrolls (`MAX_LIST_HEIGHT` 420 in the capture).
const LIST_HEIGHT: f32 = 420.;
/// The message length limit without Nitro, for options that carry a message (`getMaxMessageLength`).
const MESSAGE_LIMIT: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Builtin {
    Ban,
    Gif,
    Kick,
    Leave,
    Me,
    Msg,
    Nick,
    Shrug,
    Spoiler,
    Tableflip,
    Thread,
    Timeout,
    Unflip,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Opt {
    pub name: String,
    pub description: String,
    pub kind: u64,
    pub required: bool,
    pub choices: Vec<(String, Value)>,
    pub max_length: Option<usize>,
    pub options: Vec<Opt>,
}

impl Opt {
    fn new(name: &str, kind: u64, description: &str, required: bool) -> Self {
        Self {
            name: name.to_owned(),
            description: description.to_owned(),
            kind,
            required,
            choices: Vec::new(),
            max_length: None,
            options: Vec::new(),
        }
    }

    fn from_value(value: &Value) -> Self {
        Self {
            name: value["name"].as_str().unwrap_or_default().to_owned(),
            description: value["description"].as_str().unwrap_or_default().to_owned(),
            kind: value["type"].as_u64().unwrap_or(STRING),
            required: value["required"].as_bool().unwrap_or(false),
            choices: value["choices"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|choice| {
                    (
                        choice["name"].as_str().unwrap_or_default().to_owned(),
                        choice["value"].clone(),
                    )
                })
                .collect(),
            max_length: value["max_length"].as_u64().map(|n| n as usize),
            options: value["options"]
                .as_array()
                .into_iter()
                .flatten()
                .map(Self::from_value)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Cmd {
    pub id: String,
    pub name: String,
    pub description: String,
    pub app_id: String,
    pub app_name: String,
    pub icon: Option<String>,
    pub built_in: Option<Builtin>,
    pub options: Vec<Opt>,
    /// The index entry as it came, which an interaction repeats as `application_command`.
    pub raw: Value,
}

/// What the account may do where the list is shown (the `predicate`s of the built-ins).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct CommandScope {
    pub in_guild: bool,
    pub group_dm: bool,
    pub can_kick: bool,
    pub can_ban: bool,
    pub can_moderate: bool,
    pub can_nick: bool,
    pub can_thread: bool,
}

fn built_in(id: i32, name: &str, which: Builtin, description: &str, options: Vec<Opt>) -> Cmd {
    Cmd {
        id: id.to_string(),
        name: name.to_owned(),
        description: description.to_owned(),
        app_id: BUILT_IN_APP.to_owned(),
        app_name: "Built-In".to_owned(),
        icon: None,
        built_in: Some(which),
        options,
        raw: Value::Null,
    }
}

fn message_option(description: &str, required: bool) -> Opt {
    let mut option = Opt::new("message", STRING, description, required);
    option.max_length = Some(MESSAGE_LIMIT);
    option
}

/// Modules 12115 and 743361 (`getBuiltInCommands`), filtered by each command's predicate, in name order.
pub(super) fn built_in_commands(ctx: CommandScope) -> Vec<Cmd> {
    let hours = |label: &str, seconds: i64| (label.to_owned(), json!(seconds));
    let mut ban_days = Opt::new(
        "delete_messages",
        INTEGER,
        "How much of their recent message history to delete",
        true,
    );
    ban_days.choices = vec![
        hours("Don't Delete Any", 0),
        hours("Previous Hour", 3600),
        hours("Previous 6 Hours", 6 * 3600),
        hours("Previous 12 Hours", 12 * 3600),
        hours("Previous 24 Hours", 86400),
        hours("Previous 3 Days", 3 * 86400),
        hours("Previous 7 Days", 7 * 86400),
    ];
    let mut duration = Opt::new(
        "duration",
        INTEGER,
        "How long they should be timed out for",
        true,
    );
    duration.choices = vec![
        hours("60 secs", 60),
        hours("5 mins", 300),
        hours("10 mins", 600),
        hours("1 hour", 3600),
        hours("1 day", 86400),
        hours("1 week", 604800),
    ];
    let mut out = Vec::new();
    let mut add = |command: Cmd| out.push(command);
    if ctx.in_guild && ctx.can_ban {
        add(built_in(
            -12,
            "ban",
            Builtin::Ban,
            "Ban user",
            vec![
                Opt::new("user", USER, "The user to ban", true),
                ban_days,
                Opt::new("reason", STRING, "The reason for banning, if any", false),
            ],
        ));
    }
    add(built_in(
        -16,
        "gif",
        Builtin::Gif,
        "Search Animated GIFs on the Web",
        vec![Opt::new("query", STRING, "Search for a GIF", true)],
    ));
    if ctx.in_guild && ctx.can_kick {
        add(built_in(
            -11,
            "kick",
            Builtin::Kick,
            "Kick user",
            vec![
                Opt::new("user", USER, "The user to kick", true),
                Opt::new("reason", STRING, "The reason for kicking, if any", false),
            ],
        ));
    }
    if ctx.group_dm {
        add(built_in(
            -15,
            "leave",
            Builtin::Leave,
            "Leave group",
            vec![Opt::new(
                "silent",
                BOOLEAN,
                "Leave without notifying other members",
                false,
            )],
        ));
    }
    add(built_in(
        -5,
        "me",
        Builtin::Me,
        "Displays text with emphasis.",
        vec![message_option("Your message", true)],
    ));
    add(built_in(
        -14,
        "msg",
        Builtin::Msg,
        "Message user",
        vec![
            Opt::new("user", USER, "The user to message", true),
            message_option("Message to send", true),
        ],
    ));
    if ctx.in_guild && ctx.can_nick {
        let mut nick = Opt::new("new_nick", STRING, "New nickname", false);
        nick.max_length = Some(32);
        add(built_in(
            -7,
            "nick",
            Builtin::Nick,
            "Change nickname on this server.",
            vec![nick],
        ));
    }
    add(built_in(
        -1,
        "shrug",
        Builtin::Shrug,
        "Appends ¯\\_(ツ)_/¯ to your message.",
        vec![message_option("Your message", false)],
    ));
    add(built_in(
        -6,
        "spoiler",
        Builtin::Spoiler,
        "Marks your message as a spoiler.",
        vec![message_option("Your message", true)],
    ));
    add(built_in(
        -2,
        "tableflip",
        Builtin::Tableflip,
        "Appends (╯°□°)╯︵ ┻━┻ to your message.",
        vec![message_option("Your message", false)],
    ));
    if ctx.in_guild && ctx.can_thread {
        let mut name = Opt::new("name", STRING, "Type a name for your thread", true);
        name.max_length = Some(100);
        add(built_in(
            -10,
            "thread",
            Builtin::Thread,
            "Start new thread",
            vec![
                name,
                message_option("Type the first message in your thread", true),
            ],
        ));
    }
    if ctx.in_guild && ctx.can_moderate {
        add(built_in(
            -13,
            "timeout",
            Builtin::Timeout,
            "Timeout user",
            vec![
                Opt::new("user", USER, "The user to timeout", true),
                duration,
                Opt::new(
                    "reason",
                    STRING,
                    "The reason for timing them out, if any",
                    false,
                ),
            ],
        ));
    }
    add(built_in(
        -3,
        "unflip",
        Builtin::Unflip,
        "Appends ┬─┬ ノ( ゜-゜ノ) to your message.",
        vec![message_option("Your message", false)],
    ));
    out
}

/// The chat-input commands of an index body, with their applications (`type: 1` or absent).
pub(super) fn index_commands(index: &Value) -> Vec<Cmd> {
    let applications = index["applications"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    index["application_commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|command| command["type"].as_u64().unwrap_or(1) == 1)
        .map(|command| {
            let app_id = command["application_id"].as_str().unwrap_or_default();
            let application = applications
                .iter()
                .find(|application| application["id"].as_str() == Some(app_id));
            Cmd {
                id: command["id"].as_str().unwrap_or_default().to_owned(),
                name: command["name"].as_str().unwrap_or_default().to_owned(),
                description: command["description"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                app_id: app_id.to_owned(),
                app_name: application
                    .and_then(|application| application["name"].as_str())
                    .unwrap_or_default()
                    .to_owned(),
                icon: application
                    .and_then(|application| application["icon"].as_str())
                    .map(str::to_owned),
                built_in: None,
                options: command["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(Opt::from_value)
                    .collect(),
                raw: command.clone(),
            }
        })
        .collect()
}

/// The commands a query lists: names starting with it, then names containing it, then app names. Grouped by
/// application in the order they come (apps first, the built-ins last), each group sorted by name.
pub(super) fn matching(commands: &[Cmd], query: &str) -> Vec<Cmd> {
    let query = query.to_lowercase();
    let rank = |command: &Cmd| -> Option<u8> {
        let name = command.name.to_lowercase();
        if query.is_empty() || name.starts_with(&query) {
            Some(0)
        } else if name.contains(&query) {
            Some(1)
        } else if command.app_name.to_lowercase().contains(&query) {
            Some(2)
        } else {
            None
        }
    };
    let mut hits: Vec<(u8, Cmd)> = commands
        .iter()
        .filter_map(|command| rank(command).map(|rank| (rank, command.clone())))
        .collect();
    hits.sort_by(|(a_rank, a), (b_rank, b)| {
        let built_in = |command: &Cmd| command.app_id == BUILT_IN_APP;
        built_in(a)
            .cmp(&built_in(b))
            .then_with(|| a.app_id.cmp(&b.app_id))
            .then_with(|| a_rank.cmp(b_rank))
            .then_with(|| a.name.cmp(&b.name))
    });
    hits.into_iter().map(|(_, command)| command).collect()
}

/// A value the user filled in, with how it is shown in its chip.
#[derive(Clone, Debug)]
pub(super) struct Filled {
    pub opt: Opt,
    pub value: Value,
    pub shown: String,
}

/// A command whose options are being filled in the composer.
pub(super) struct Pending {
    pub cmd: Cmd,
    pub filled: Vec<Filled>,
    pub selected: usize,
}

/// The options now being asked: the command's, or those of the sub-command or group chosen so far.
pub(super) fn active_options<'a>(cmd: &'a Cmd, filled: &[Filled]) -> &'a [Opt] {
    let mut options: &[Opt] = &cmd.options;
    for step in filled {
        if matches!(step.opt.kind, SUB_COMMAND | SUB_COMMAND_GROUP)
            && let Some(found) = options.iter().find(|option| option.name == step.opt.name)
        {
            options = &found.options;
        }
    }
    options
}

/// The option to ask next: a sub-command to choose, or the first option not filled yet.
pub(super) fn current_option(cmd: &Cmd, filled: &[Filled]) -> Option<Opt> {
    let options = active_options(cmd, filled);
    if options
        .iter()
        .any(|option| matches!(option.kind, SUB_COMMAND | SUB_COMMAND_GROUP))
    {
        return Some(Opt {
            name: "subcommand".to_owned(),
            description: String::new(),
            kind: SUB_COMMAND,
            required: true,
            choices: options
                .iter()
                .map(|option| (option.name.clone(), json!(option.name)))
                .collect(),
            max_length: None,
            options: Vec::new(),
        });
    }
    options
        .iter()
        .find(|option| !filled.iter().any(|step| step.opt.name == option.name))
        .cloned()
}

fn missing_required(cmd: &Cmd, filled: &[Filled]) -> Option<Opt> {
    let options = active_options(cmd, filled);
    if options
        .iter()
        .any(|option| matches!(option.kind, SUB_COMMAND | SUB_COMMAND_GROUP))
    {
        return current_option(cmd, filled);
    }
    options
        .iter()
        .find(|option| option.required && !filled.iter().any(|step| step.opt.name == option.name))
        .cloned()
}

/// `options` of an interaction: a chosen sub-command (and group) nests the options filled after it.
pub(super) fn nested_options(filled: &[Filled]) -> Vec<Value> {
    fn build(filled: &[Filled]) -> Vec<Value> {
        let mut out = Vec::new();
        let mut index = 0;
        while index < filled.len() {
            let step = &filled[index];
            if matches!(step.opt.kind, SUB_COMMAND | SUB_COMMAND_GROUP) {
                out.push(json!({
                    "type": step.opt.kind,
                    "name": step.opt.name,
                    "options": build(&filled[index + 1..]),
                }));
                break;
            }
            out.push(json!({"type": step.opt.kind, "name": step.opt.name, "value": step.value}));
            index += 1;
        }
        out
    }
    build(filled)
}

/// The body of `POST /interactions` for a chat-input command (module 12115's `runCommand` path).
pub(super) fn interaction_body(
    cmd: &Cmd,
    filled: &[Filled],
    guild: Option<Id>,
    channel: Id,
    session: &str,
    nonce: &str,
) -> Value {
    json!({
        "type": 2,
        "application_id": cmd.app_id,
        "guild_id": guild.map(|guild| guild.to_string()),
        "channel_id": channel.to_string(),
        "session_id": session,
        "data": {
            "version": cmd.raw["version"],
            "id": cmd.id,
            "name": cmd.name,
            "type": 1,
            "options": nested_options(filled),
            "application_command": cmd.raw,
            "attachments": [],
        },
        "nonce": nonce,
        "analytics_location": "slash_ui",
    })
}

fn string_value(filled: &[Filled], name: &str) -> String {
    filled
        .iter()
        .find(|step| step.opt.name == name)
        .and_then(|step| step.value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The message a text built-in sends (`execute` of shrug, tableflip, unflip, me and spoiler).
pub(super) fn text_content(which: Builtin, message: &str) -> Option<String> {
    let text = match which {
        Builtin::Shrug => format!("{message} ¯\\_(ツ)_/¯"),
        Builtin::Tableflip => format!("{message} (╯°□°)╯︵ ┻━┻"),
        Builtin::Unflip => format!("{message} ┬─┬ノ( º _ ºノ)"),
        Builtin::Me => format!("_{message}_"),
        Builtin::Spoiler => format!("||{message}||"),
        _ => return None,
    };
    Some(text.trim().to_owned())
}

/// What is typed into an option, as the value it stands for; `Err` is a reason shown to the person.
pub(super) fn parse_value(opt: &Opt, text: &str) -> Result<(Value, String), &'static str> {
    let text = text.trim();
    if text.is_empty() && opt.required {
        return Err("This field is required");
    }
    match opt.kind {
        STRING => {
            if let Some(max) = opt.max_length
                && text.chars().count() > max
            {
                return Err("Too long");
            }
            Ok((json!(text), text.to_owned()))
        }
        INTEGER => text
            .parse::<i64>()
            .map(|n| (json!(n), n.to_string()))
            .map_err(|_| "Enter a whole number"),
        NUMBER => text
            .parse::<f64>()
            .map(|n| (json!(n), n.to_string()))
            .map_err(|_| "Enter a number"),
        ATTACHMENT => Err("Files cannot be given to a command here yet"),
        _ => Err("Choose one from the list"),
    }
}

/// One line the option list offers while an option is being chosen.
#[derive(Clone, Debug)]
pub(super) struct Choice {
    pub label: String,
    pub value: Value,
}

pub(super) struct List {
    pub commands: Vec<Cmd>,
    pub selected: usize,
    pub scroll: ScrollHandle,
    pub query: String,
}

pub(super) struct Launcher {
    pub search: Entity<Editor>,
}

#[derive(Default)]
pub(super) struct State {
    pub list: Option<List>,
    pub pending: Option<Pending>,
    pub launcher: Option<Launcher>,
    /// Applications whose commands were used, newest first (the launcher's Recents).
    pub recents: Vec<(String, Option<String>, String)>,
    /// A reason the last Enter did not run the command.
    pub notice: Option<String>,
    /// `/gif` ran: the key handler opens the picker on its GIF tab, which needs the window.
    pub open_gif: bool,
}

impl Root {
    fn slash_context(&self) -> CommandScope {
        let guild = self.current_guild();
        let channel = self.current_channel().cloned();
        let me = self.stores.me();
        let can = |permission: u64| match (guild, &channel, me) {
            (Some(guild), Some(channel), Some(me)) => {
                permissions::can_in_channel(guild, channel, me, permission)
            }
            _ => false,
        };
        CommandScope {
            in_guild: guild.is_some(),
            group_dm: self.current_dm().is_some_and(|dm| dm.kind == 3),
            can_kick: can(permissions::KICK_MEMBERS),
            can_ban: can(permissions::BAN_MEMBERS),
            can_moderate: can(permissions::MODERATE_MEMBERS),
            can_nick: can(permissions::CHANGE_NICKNAME) || can(permissions::MANAGE_NICKNAMES),
            can_thread: channel.as_ref().is_some_and(|channel| {
                matches!(channel.kind, ChannelKind::Text | ChannelKind::Other(5))
            }),
        }
    }

    /// Everything the `/` list can offer here: the index's app commands, then the built-ins.
    pub(super) fn all_commands(&self) -> Vec<Cmd> {
        let mut commands = self
            .command_index_body()
            .map(|index| index_commands(&index))
            .unwrap_or_default();
        commands.extend(built_in_commands(self.slash_context()));
        commands
    }

    /// Rebuilds the `/` list for what is typed. Called by the autocomplete refresh with the text after the slash.
    pub(super) fn refresh_slash(&mut self, query: Option<&str>, cx: &mut Context<Self>) {
        let query = query.filter(|_| self.slash.pending.is_none());
        let Some(query) = query else {
            if self.slash.list.take().is_some() {
                cx.notify();
            }
            return;
        };
        self.request_command_index();
        let commands = matching(&self.all_commands(), query);
        for command in &commands {
            self.request_command_icon(command);
        }
        if commands.is_empty() {
            self.slash.list = None;
            cx.notify();
            return;
        }
        let (selected, scroll) = match self.slash.list.take() {
            Some(old) if old.query == query => (old.selected.min(commands.len() - 1), old.scroll),
            _ => (0, ScrollHandle::new()),
        };
        self.slash.list = Some(List {
            commands,
            selected,
            scroll,
            query: query.to_owned(),
        });
        cx.notify();
    }

    fn request_command_icon(&mut self, command: &Cmd) {
        if let Some(hash) = &command.icon {
            let key = format!("app-icon:{}:{hash}", command.app_id);
            self.request_image(
                key,
                format!(
                    "https://cdn.discordapp.com/app-icons/{}/{hash}.png?size=64",
                    command.app_id
                ),
            );
        }
    }

    /// `/name ` typed out in full becomes the command's chip, as in the official editor.
    pub(super) fn slash_convert(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        if self.slash.pending.is_some() {
            return false;
        }
        let Some(rest) = text.strip_prefix('/') else {
            return false;
        };
        let Some((name, remainder)) = rest.split_once(' ') else {
            return false;
        };
        let commands = self.all_commands();
        let Some(found) = commands
            .into_iter()
            .find(|command| command.name.eq_ignore_ascii_case(name))
        else {
            return false;
        };
        self.begin_command(found, remainder, cx);
        true
    }

    /// Puts the command's chip in the composer; `text` is what already stands for its first option.
    pub(super) fn begin_command(&mut self, cmd: Cmd, text: &str, cx: &mut Context<Self>) {
        self.slash.list = None;
        self.slash.notice = None;
        self.slash.pending = Some(Pending {
            cmd,
            filled: Vec::new(),
            selected: 0,
        });
        self.autocomplete.open = None;
        let text = text.to_owned();
        self.composer
            .update(cx, |composer, _| composer.set_text(&text));
        cx.notify();
    }

    /// The people, channels, roles and fixed answers the current option can take, filtered by what is typed.
    pub(super) fn option_choices(&self, opt: &Opt, query: &str) -> Vec<Choice> {
        let query = query.trim().to_lowercase();
        let keep = |label: &str| query.is_empty() || label.to_lowercase().contains(&query);
        let mut out: Vec<Choice> = Vec::new();
        if !opt.choices.is_empty() {
            out.extend(opt.choices.iter().filter(|(label, _)| keep(label)).map(
                |(label, value)| Choice {
                    label: label.clone(),
                    value: value.clone(),
                },
            ));
            return out;
        }
        match opt.kind {
            BOOLEAN => {
                for (label, value) in [("true", true), ("false", false)] {
                    if keep(label) {
                        out.push(Choice {
                            label: label.to_owned(),
                            value: json!(value),
                        });
                    }
                }
            }
            USER | MENTIONABLE => {
                for (user, nick) in self.mention_candidates() {
                    let label = nick.unwrap_or_else(|| user.display_name().to_owned());
                    if keep(&label) || keep(&user.username) {
                        out.push(Choice {
                            label,
                            value: json!(user.id.to_string()),
                        });
                    }
                }
                if opt.kind == MENTIONABLE {
                    out.extend(self.role_choices(&query));
                }
            }
            ROLE => out.extend(self.role_choices(&query)),
            CHANNEL => {
                if let Some(guild) = self.current_guild() {
                    let mut channels: Vec<&Channel> = guild
                        .channels
                        .iter()
                        .filter(|channel| channel.kind != ChannelKind::Category)
                        .filter(|channel| keep(&channel.name))
                        .collect();
                    channels.sort_by_key(|channel| channel.position);
                    out.extend(channels.into_iter().take(25).map(|channel| Choice {
                        label: channel.name.clone(),
                        value: json!(channel.id.to_string()),
                    }));
                }
            }
            _ => {}
        }
        out.truncate(25);
        out
    }

    fn role_choices(&self, query: &str) -> Vec<Choice> {
        let Some(guild) = self.current_guild() else {
            return Vec::new();
        };
        let mut roles: Vec<(&Id, &String)> = guild
            .roles
            .iter()
            .filter(|(id, name)| **id != guild.id && name.to_lowercase().contains(query))
            .collect();
        roles.sort_by(|a, b| a.1.cmp(b.1));
        roles
            .into_iter()
            .map(|(id, name)| Choice {
                label: name.clone(),
                value: json!(id.to_string()),
            })
            .collect()
    }

    /// The choices of the option being asked now, for the popup and the keys.
    pub(super) fn pending_choices(&self, cx: &Context<Self>) -> Vec<Choice> {
        let Some(pending) = &self.slash.pending else {
            return Vec::new();
        };
        let Some(opt) = current_option(&pending.cmd, &pending.filled) else {
            return Vec::new();
        };
        self.option_choices(&opt, self.composer.read(cx).text())
    }

    /// Fills the current option with `value` and asks the next one, or runs the command when nothing required is left
    /// and `run` is set.
    fn fill_option(&mut self, value: Value, shown: String, run: bool, cx: &mut Context<Self>) {
        let Some(pending) = self.slash.pending.as_mut() else {
            return;
        };
        let Some(opt) = current_option(&pending.cmd, &pending.filled) else {
            return;
        };
        // A chosen sub-command (or group) is recorded as the option it names.
        let opt = if opt.kind == SUB_COMMAND && opt.name == "subcommand" {
            let chosen = value.as_str().unwrap_or_default();
            active_options(&pending.cmd, &pending.filled)
                .iter()
                .find(|option| option.name == chosen)
                .cloned()
                .unwrap_or(opt)
        } else {
            opt
        };
        pending.filled.push(Filled { opt, value, shown });
        pending.selected = 0;
        self.slash.notice = None;
        self.composer
            .update(cx, |composer, _| composer.set_text(""));
        if run
            && let Some(pending) = &self.slash.pending
            && missing_required(&pending.cmd, &pending.filled).is_none()
        {
            self.run_pending(cx);
        }
        cx.notify();
    }

    /// Enter and Tab: accept the highlighted choice, or the text typed for a free option. `run` is Enter.
    pub(super) fn advance_pending(&mut self, run: bool, cx: &mut Context<Self>) {
        let Some(pending) = self.slash.pending.as_ref() else {
            return;
        };
        let text = self.composer.read(cx).text().to_owned();
        let current = current_option(&pending.cmd, &pending.filled);
        let Some(opt) = current else {
            if run {
                self.run_pending(cx);
            }
            return;
        };
        let needs_pick = matches!(
            opt.kind,
            SUB_COMMAND | USER | CHANNEL | ROLE | MENTIONABLE | BOOLEAN
        ) || !opt.choices.is_empty();
        if needs_pick {
            let choices = self.option_choices(&opt, &text);
            let at = pending.selected.min(choices.len().saturating_sub(1));
            if let Some(choice) = choices.get(at) {
                let (value, shown) = (choice.value.clone(), choice.label.clone());
                self.fill_option(value, shown, run, cx);
            } else {
                self.slash.notice = Some("No match".to_owned());
                cx.notify();
            }
            return;
        }
        if text.trim().is_empty() && !opt.required {
            // An optional option left empty is skipped.
            if let Some(pending) = self.slash.pending.as_mut() {
                pending.filled.push(Filled {
                    opt,
                    value: Value::Null,
                    shown: String::new(),
                });
            }
            if run {
                self.finish_if_ready(cx);
            }
            cx.notify();
            return;
        }
        match parse_value(&opt, &text) {
            Ok((value, shown)) => self.fill_option(value, shown, run, cx),
            Err(reason) => {
                self.slash.notice = Some(reason.to_owned());
                cx.notify();
            }
        }
    }

    fn finish_if_ready(&mut self, cx: &mut Context<Self>) {
        if let Some(pending) = &self.slash.pending
            && missing_required(&pending.cmd, &pending.filled).is_none()
        {
            self.run_pending(cx);
        }
    }

    /// Enter with the command complete (the composer is empty or holds the last option's text).
    pub(super) fn submit_pending(&mut self, cx: &mut Context<Self>) {
        self.advance_pending(true, cx);
        if self.slash.pending.is_some() {
            self.finish_if_ready(cx);
        }
    }

    /// Backspace on an empty composer takes the last chip back into the text, or drops the command.
    pub(super) fn back_pending(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.slash.pending.as_mut() else {
            return;
        };
        match pending.filled.pop() {
            Some(step) => {
                let text = step.shown;
                self.composer
                    .update(cx, |composer, _| composer.set_text(&text));
            }
            None => self.slash.pending = None,
        }
        self.slash.notice = None;
        cx.notify();
    }

    pub(super) fn cancel_pending(&mut self, cx: &mut Context<Self>) {
        self.slash.pending = None;
        self.slash.notice = None;
        self.composer
            .update(cx, |composer, _| composer.set_text(""));
        cx.notify();
    }

    /// Sends the command: an interaction for an app's, the built-in's own effect otherwise.
    fn run_pending(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.slash.pending.take() else {
            return;
        };
        let filled: Vec<Filled> = pending
            .filled
            .iter()
            .filter(|step| !step.value.is_null())
            .cloned()
            .collect();
        let cmd = pending.cmd;
        self.composer
            .update(cx, |composer, _| composer.set_text(""));
        self.slash.notice = None;
        let Some(channel) = self.channel else {
            return;
        };
        let entry = (cmd.app_id.clone(), cmd.icon.clone(), cmd.app_name.clone());
        self.slash.recents.retain(|known| known.0 != entry.0);
        self.slash.recents.insert(0, entry);
        self.slash.recents.truncate(8);
        match cmd.built_in {
            Some(which) => self.run_builtin(which, &filled, channel, cx),
            None => {
                let session = self.stores.session_id.clone().unwrap_or_default();
                let body = interaction_body(
                    &cmd,
                    &filled,
                    self.guild,
                    channel,
                    &session,
                    &crate::net::nonce(),
                );
                let _ = self.requests.send(Request::Interaction { body });
            }
        }
        cx.notify();
    }

    fn run_builtin(
        &mut self,
        which: Builtin,
        filled: &[Filled],
        channel: Id,
        cx: &mut Context<Self>,
    ) {
        let message = string_value(filled, "message");
        if let Some(content) = text_content(which, &message) {
            self.last_typing_sent = None;
            self.send(content);
            return;
        }
        let guild = self.guild;
        let user = string_value(filled, "user");
        let reason = string_value(filled, "reason");
        let rest =
            |this: &mut Self, method, route: String, body: Option<Value>, key: Option<String>| {
                let _ = this.requests.send(Request::Rest {
                    key,
                    method,
                    route,
                    body,
                });
            };
        match (which, guild) {
            (Builtin::Nick, Some(guild)) => {
                let nick = string_value(filled, "new_nick");
                rest(
                    self,
                    reqwest::Method::PATCH,
                    format!("/guilds/{guild}/members/@me"),
                    Some(json!({"nick": if nick.is_empty() { Value::Null } else { json!(nick) }})),
                    None,
                );
            }
            (Builtin::Kick, Some(guild)) => {
                rest(
                    self,
                    reqwest::Method::DELETE,
                    format!("/guilds/{guild}/members/{user}"),
                    None,
                    Some(format!("{ANSWER}kick:{channel}")),
                );
                let _ = reason;
            }
            (Builtin::Ban, Some(guild)) => {
                let seconds = filled
                    .iter()
                    .find(|step| step.opt.name == "delete_messages")
                    .and_then(|step| step.value.as_i64())
                    .unwrap_or(0);
                rest(
                    self,
                    reqwest::Method::PUT,
                    format!("/guilds/{guild}/bans/{user}"),
                    Some(json!({"delete_message_seconds": seconds})),
                    Some(format!("{ANSWER}ban:{channel}")),
                );
            }
            (Builtin::Timeout, Some(guild)) => {
                let seconds = filled
                    .iter()
                    .find(|step| step.opt.name == "duration")
                    .and_then(|step| step.value.as_i64())
                    .unwrap_or(60);
                let until = chrono::Utc::now() + chrono::Duration::seconds(seconds);
                rest(
                    self,
                    reqwest::Method::PATCH,
                    format!("/guilds/{guild}/members/{user}"),
                    Some(json!({
                        "communication_disabled_until":
                            until.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                    })),
                    Some(format!("{ANSWER}timeout:{channel}")),
                );
            }
            (Builtin::Thread, Some(_)) => {
                let name = string_value(filled, "name");
                rest(
                    self,
                    reqwest::Method::POST,
                    format!("/channels/{channel}/threads"),
                    Some(json!({
                        "name": name,
                        "type": 11,
                        "auto_archive_duration": 1440,
                        "location": "Slash Command",
                    })),
                    Some(format!("{ANSWER}thread:{message}")),
                );
            }
            (Builtin::Msg, _) => {
                rest(
                    self,
                    reqwest::Method::POST,
                    "/users/@me/channels".to_owned(),
                    Some(json!({"recipients": [user]})),
                    Some(format!("{ANSWER}msg:{message}")),
                );
            }
            (Builtin::Leave, _) => self.close_dm(channel, cx),
            (Builtin::Gif, _) => self.slash.open_gif = true,
            _ => {}
        }
    }

    /// Answers to the REST calls of the built-ins: a toast where the official posts a local bot message.
    pub(super) fn slash_fetched(
        &mut self,
        key: &str,
        report: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rest) = key.strip_prefix(ANSWER) else {
            return false;
        };
        let ok = report["ok"].as_bool().unwrap_or(false);
        let (what, extra) = rest.split_once(':').unwrap_or((rest, ""));
        match (what, ok) {
            ("kick", true) => {
                self.show_success_toast("User was kicked from the server.".to_owned(), cx)
            }
            ("kick", false) => self.show_app_toast(
                "Oops! We encountered an error when trying to kick that user.",
                cx,
            ),
            ("ban", true) => {
                self.show_success_toast("User was banned from the server.".to_owned(), cx)
            }
            ("ban", false) => self.show_app_toast(
                "Oops! We encountered an error when trying to ban that user.",
                cx,
            ),
            ("timeout", true) => self.show_success_toast("User timed out.".to_owned(), cx),
            ("timeout", false) => self.show_app_toast(
                "Oops! We encountered an error when trying to timeout that user.",
                cx,
            ),
            ("thread", true) => {
                let thread = report["body"]["id"]
                    .as_str()
                    .and_then(|id| id.parse::<u64>().ok());
                if let Some(thread) = thread {
                    let _ = self.requests.send(Request::Send {
                        channel: thread,
                        content: extra.to_owned(),
                        files: Vec::new(),
                        reply: None,
                    });
                }
            }
            ("thread", false) => self.show_app_toast(
                "Oops! We encountered an error when trying to start that thread.",
                cx,
            ),
            ("msg", true) => {
                let dm = report["body"]["id"]
                    .as_str()
                    .and_then(|id| id.parse::<u64>().ok());
                if let Some(dm) = dm {
                    self.stores.add_private_channel(&report["body"]);
                    let _ = self.requests.send(Request::Send {
                        channel: dm,
                        content: extra.to_owned(),
                        files: Vec::new(),
                        reply: None,
                    });
                }
            }
            ("msg", false) => self.show_app_toast(
                "Oops! We encountered an error when trying to message that user.",
                cx,
            ),
            _ => {}
        }
        true
    }

    /// Keys while a `/` list, an option list or a command's chips are on: the arrows move, Tab and Enter accept,
    /// Escape closes, Backspace on nothing takes a chip back.
    pub(super) fn slash_key(
        &mut self,
        keys: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let plain = !(keys.modifiers.control || keys.modifiers.alt || keys.modifiers.platform);
        if !plain {
            return false;
        }
        if let Some(list) = self.slash.list.as_mut() {
            let count = list.commands.len();
            match keys.key.as_str() {
                "up" if !keys.modifiers.shift => {
                    list.selected = (list.selected + count - 1) % count;
                    list.scroll.scroll_to_item(list.selected);
                }
                "down" if !keys.modifiers.shift => {
                    list.selected = (list.selected + 1) % count;
                    list.scroll.scroll_to_item(list.selected);
                }
                "tab" | "enter" if !keys.modifiers.shift => {
                    let at = list.selected;
                    self.accept_command(at, window, cx);
                    return true;
                }
                "escape" => {
                    self.slash.list = None;
                    self.autocomplete.hidden = Some(self.composer.read(cx).text().to_owned());
                }
                _ => return false,
            }
            cx.notify();
            return true;
        }
        if self.slash.pending.is_none() {
            return false;
        }
        match keys.key.as_str() {
            "up" | "down" => {
                let count = self.pending_choices(cx).len();
                if count == 0 {
                    return false;
                }
                if let Some(pending) = self.slash.pending.as_mut() {
                    pending.selected = if keys.key == "up" {
                        (pending.selected + count - 1) % count
                    } else {
                        (pending.selected + 1) % count
                    };
                }
            }
            "tab" if !keys.modifiers.shift => self.advance_pending(false, cx),
            "enter" if !keys.modifiers.shift => {
                self.submit_pending(cx);
                if std::mem::take(&mut self.slash.open_gif) {
                    let anchor = self.tip_anchors.borrow().get("composer-box").copied();
                    if let Some(bounds) = anchor {
                        self.open_picker_tab(
                            super::picker::Tab::Gifs,
                            gpui::point(bounds.left(), bounds.top() - px(9.)),
                            window,
                            cx,
                        );
                    }
                }
            }
            "escape" => self.cancel_pending(cx),
            "backspace" if self.composer.read(cx).text().is_empty() => self.back_pending(cx),
            _ => {
                if let Some(pending) = self.slash.pending.as_mut() {
                    pending.selected = 0;
                }
                return false;
            }
        }
        cx.notify();
        true
    }

    /// Picks a command from the list: the command's chip goes into the composer.
    pub(super) fn accept_command(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(list) = self.slash.list.take() else {
            return;
        };
        let Some(command) = list.commands.get(index).cloned() else {
            return;
        };
        self.begin_command(command, "", cx);
        window.focus(&self.composer.read(cx).focus_handle(cx), cx);
    }

    fn command_index_body(&self) -> Option<Value> {
        self.command_index_value().cloned()
    }

    /// The launcher the composer's Apps button opens.
    pub(super) fn open_launcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.slash.launcher.take().is_some() {
            cx.notify();
            return;
        }
        self.request_command_index();
        let search = cx.new(|cx| Editor::new(editor_style(), cx));
        search.update(cx, |editor, _| {
            editor.set_placeholder("Search Apps & Commands".to_owned())
        });
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&search, |this, _, event, cx| match event {
            EditorEvent::Submit(_) => {
                let query = this
                    .slash
                    .launcher
                    .as_ref()
                    .map(|launcher| launcher.search.read(cx).text().to_owned())
                    .unwrap_or_default();
                if let Some(first) = matching(&this.all_commands(), &query).into_iter().next() {
                    this.slash.launcher = None;
                    this.begin_command(first, "", cx);
                }
            }
            EditorEvent::Cancel => {
                this.slash.launcher = None;
                cx.notify();
            }
            EditorEvent::Edited => {}
        })
        .detach();
        window.focus(&search.read(cx).focus_handle(cx), cx);
        self.slash.launcher = Some(Launcher { search });
        cx.notify();
    }
}

const ANSWER: &str = "slash-answer:";

fn group_key(command: &Cmd) -> &str {
    &command.app_id
}

impl Root {
    fn command_icon(&self, command: &Cmd, size: f32) -> AnyElement {
        let picture = command.icon.as_ref().and_then(|hash| {
            self.images
                .get(&format!("app-icon:{}:{hash}", command.app_id))
                .cloned()
        });
        match picture {
            Some(picture) => img(picture)
                .size(px(size))
                .rounded(px(size / 4.))
                .flex_none()
                .into_any_element(),
            None => div()
                .size(px(size))
                .flex_none()
                .rounded(px(size / 4.))
                .bg(color("--white"))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(size * 0.6))
                .font_weight(FontWeight(700.))
                .text_color(color("--black"))
                .child("/")
                .into_any_element(),
        }
    }

    /// The `/` list: the rail of applications on the left, the commands by application on the right.
    pub(super) fn slash_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let list = self.slash.list.as_ref()?;
        let mut rail: Vec<AnyElement> = Vec::new();
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut last: Option<&str> = None;
        for (index, command) in list.commands.iter().enumerate() {
            if last != Some(group_key(command)) {
                last = Some(group_key(command));
                rail.push(
                    div()
                        .size(px(32.))
                        .mb(px(8.))
                        .child(self.command_icon(command, 32.))
                        .into_any_element(),
                );
                rows.push(
                    div()
                        .h(px(32.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(self.command_icon(command, 16.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-default"))
                                .child(command.app_name.clone()),
                        )
                        .into_any_element(),
                );
            }
            let selected = index == list.selected;
            rows.push(
                div()
                    .id(("slash-row", index))
                    .h(px(56.))
                    .px(px(8.))
                    .pt(px(8.))
                    .rounded(px(3.))
                    .cursor_pointer()
                    .when(selected, |row| row.bg(color("--background-mod-subtle")))
                    .hover(|row| row.bg(color("--background-mod-subtle")))
                    .flex()
                    .justify_between()
                    .child(
                        div()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(format!("/{}", command.name)),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .overflow_hidden()
                                    .child(command.description.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .ml(px(16.))
                            .mt(px(12.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(command.app_name.clone()),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.accept_command(index, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .into_any_element(),
            );
        }
        Some(
            div()
                .id("slash-popup")
                .absolute()
                .left(px(0.))
                .right(px(8.))
                .bottom(relative(1.))
                .mb(px(8.))
                .max_h(px(LIST_HEIGHT))
                .occlude()
                .rounded(px(5.))
                .overflow_hidden()
                .bg(color("--background-surface-high"))
                .flex()
                .child(
                    div()
                        .w(px(48.))
                        .flex_none()
                        .bg(color("--background-base-lowest"))
                        .p(px(8.))
                        .pt(px(16.))
                        .children(rail),
                )
                .child(
                    div()
                        .id("slash-scroll")
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_y_scroll()
                        .track_scroll(&list.scroll)
                        .py(px(8.))
                        .pl(px(8.))
                        .pr(px(16.))
                        .children(rows),
                )
                .into_any_element(),
        )
    }

    /// The chips in front of the editor: the command, then each option as `name value`.
    pub(super) fn pending_chips(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let pending = self.slash.pending.as_ref()?;
        let chip = |text: String, strong: bool| {
            div()
                .h(px(22.))
                .px(px(6.))
                .mr(px(4.))
                .rounded(px(3.))
                .flex()
                .items_center()
                .text_size(px(14.))
                .when(strong, |chip| {
                    chip.bg(color("--background-mod-strong"))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                })
                .when(!strong, |chip| {
                    chip.bg(color("--background-mod-subtle"))
                        .text_color(color("--text-default"))
                })
                .child(text)
        };
        let mut chips = vec![chip(format!("/{}", pending.cmd.name), true).into_any_element()];
        for step in &pending.filled {
            if matches!(step.opt.kind, SUB_COMMAND | SUB_COMMAND_GROUP) {
                chips.push(chip(step.opt.name.clone(), true).into_any_element());
                continue;
            }
            if step.value.is_null() {
                continue;
            }
            chips.push(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .mr(px(4.))
                            .text_size(px(14.))
                            .text_color(color("--text-muted"))
                            .child(step.opt.name.clone()),
                    )
                    .child(chip(step.shown.clone(), false))
                    .into_any_element(),
            );
        }
        if let Some(opt) = current_option(&pending.cmd, &pending.filled)
            && !matches!(opt.kind, SUB_COMMAND)
        {
            chips.push(
                div()
                    .mr(px(4.))
                    .text_size(px(14.))
                    .text_color(color("--text-muted"))
                    .child(opt.name.clone())
                    .into_any_element(),
            );
        }
        let _ = cx;
        Some(
            div()
                .flex()
                .items_center()
                .h(px(22.))
                .children(chips)
                .into_any_element(),
        )
    }

    /// The list under the chips while an option takes a pick, with the option's description and any notice.
    pub(super) fn option_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pending = self.slash.pending.as_ref()?;
        let opt = current_option(&pending.cmd, &pending.filled)?;
        let choices = self.option_choices(&opt, self.composer.read(cx).text());
        let hint = if self.slash.notice.is_some() {
            self.slash.notice.clone()
        } else if opt.description.is_empty() {
            None
        } else {
            Some(opt.description.clone())
        };
        if choices.is_empty() && hint.is_none() {
            return None;
        }
        let rows: Vec<AnyElement> = choices
            .into_iter()
            .enumerate()
            .map(|(index, choice)| {
                let selected = index == pending.selected;
                let value = choice.value.clone();
                let shown = choice.label.clone();
                div()
                    .id(("slash-choice", index))
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(px(3.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .when(selected, |row| row.bg(color("--background-mod-subtle")))
                    .hover(|row| row.bg(color("--background-mod-subtle")))
                    .child(choice.label)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.fill_option(value.clone(), shown.clone(), false, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .into_any_element()
            })
            .collect();
        Some(
            div()
                .id("slash-options")
                .absolute()
                .left(px(0.))
                .right(px(8.))
                .bottom(relative(1.))
                .mb(px(8.))
                .max_h(px(LIST_HEIGHT))
                .occlude()
                .rounded(px(5.))
                .bg(color("--background-surface-high"))
                .p(px(8.))
                .child(
                    div()
                        .h(px(32.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(opt.name.clone()),
                )
                .children(hint.map(|hint| {
                    div()
                        .px(px(8.))
                        .pb(px(8.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-muted"))
                        .child(hint)
                }))
                .child(
                    div()
                        .id("slash-options-scroll")
                        .max_h(px(300.))
                        .overflow_y_scroll()
                        .children(rows),
                )
                .into_any_element(),
        )
    }

    /// The Apps button's launcher: a search field, the apps used lately, and the apps with commands here.
    pub(super) fn launcher_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let launcher = self.slash.launcher.as_ref()?;
        let query = launcher.search.read(cx).text().to_owned();
        let commands = matching(&self.all_commands(), &query);
        let mut body: Vec<AnyElement> = Vec::new();
        let heading = |text: &str| {
            div()
                .pt(px(16.))
                .pb(px(8.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text.to_owned())
        };
        if query.is_empty() {
            if !self.slash.recents.is_empty() {
                body.push(heading("Recents").into_any_element());
                body.push(
                    div()
                        .flex()
                        .gap(px(8.))
                        .children(self.slash.recents.iter().map(|(app, icon, name)| {
                            let command = Cmd {
                                id: String::new(),
                                name: String::new(),
                                description: String::new(),
                                app_id: app.clone(),
                                app_name: name.clone(),
                                icon: icon.clone(),
                                built_in: None,
                                options: Vec::new(),
                                raw: Value::Null,
                            };
                            self.command_icon(&command, 48.)
                        }))
                        .into_any_element(),
                );
            }
            let mut seen: Vec<&str> = Vec::new();
            let mut apps: Vec<AnyElement> = Vec::new();
            for command in commands.iter().filter(|command| command.built_in.is_none()) {
                if seen.contains(&command.app_id.as_str()) {
                    continue;
                }
                seen.push(&command.app_id);
                let count = commands
                    .iter()
                    .filter(|other| other.app_id == command.app_id)
                    .count();
                let name = command.app_name.clone();
                let first = commands
                    .iter()
                    .position(|other| other.app_id == command.app_id)
                    .unwrap_or(0);
                let target = commands[first].clone();
                apps.push(
                    div()
                        .id(("launcher-app", apps.len()))
                        .w(px(224.))
                        .h(px(56.))
                        .rounded(px(8.))
                        .bg(color("--background-surface-high"))
                        .p(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .cursor_pointer()
                        .child(self.command_icon(command, 40.))
                        .child(
                            div()
                                .min_w(px(0.))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-muted"))
                                        .child(format!(
                                            "{count} command{}",
                                            if count == 1 { "" } else { "s" }
                                        )),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.slash.launcher = None;
                            this.composer
                                .update(cx, |composer, _| composer.set_text("/"));
                            window.focus(&this.composer.read(cx).focus_handle(cx), cx);
                            let _ = &target;
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
            if !apps.is_empty() {
                body.push(heading("Apps").into_any_element());
                body.push(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .children(apps)
                        .into_any_element(),
                );
            }
        } else {
            for (index, command) in commands.iter().enumerate().take(60) {
                let target = command.clone();
                body.push(
                    div()
                        .id(("launcher-command", index))
                        .h(px(56.))
                        .px(px(8.))
                        .pt(px(8.))
                        .rounded(px(3.))
                        .cursor_pointer()
                        .hover(|row| row.bg(color("--background-mod-subtle")))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(format!("/{}", command.name)),
                        )
                        .child(
                            div()
                                .mt(px(4.))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-muted"))
                                .child(format!("{} - {}", command.app_name, command.description)),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.slash.launcher = None;
                            this.begin_command(target.clone(), "", cx);
                            window.focus(&this.composer.read(cx).focus_handle(cx), cx);
                        }))
                        .into_any_element(),
                );
            }
            if commands.is_empty() {
                body.push(
                    div()
                        .pt(px(24.))
                        .text_size(px(14.))
                        .text_color(color("--text-muted"))
                        .child("No results")
                        .into_any_element(),
                );
            }
        }
        Some(
            div()
                .id("launcher")
                .absolute()
                .right(px(13.))
                .bottom(px(32.))
                .w(px(500.))
                .h(px(680.))
                .occlude()
                .rounded(px(8.))
                .bg(color("--background-base-lower"))
                .border_1()
                .border_color(gpui::hsla(0., 0., 1., 0.08))
                .flex()
                .flex_col()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.slash.launcher = None;
                    cx.notify();
                }))
                .child(
                    div().p(px(16.)).child(
                        div()
                            .h(px(40.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .bg(gpui::hsla(0., 0., 0., 0.12))
                            .border_1()
                            .border_color(color("--border-focus"))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(icon("search", 16., "--interactive-text-default"))
                            .child(div().flex_1().child(launcher.search.clone())),
                    ),
                )
                .child(
                    div()
                        .id("launcher-scroll")
                        .flex_1()
                        .overflow_y_scroll()
                        .px(px(16.))
                        .pb(px(16.))
                        .children(body),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn everything() -> CommandScope {
        CommandScope {
            in_guild: true,
            group_dm: false,
            can_kick: true,
            can_ban: true,
            can_moderate: true,
            can_nick: true,
            can_thread: true,
        }
    }

    fn names(commands: &[Cmd]) -> Vec<&str> {
        commands
            .iter()
            .map(|command| command.name.as_str())
            .collect()
    }

    fn index() -> Value {
        json!({
            "applications": [{"id": "5", "name": "Wordle", "icon": "abc"}],
            "application_commands": [
                {"id": "10", "application_id": "5", "name": "wordle", "type": 1, "version": "99",
                 "description": "Play the official Wordle from New York Times Games", "options": []},
                {"id": "11", "application_id": "5", "name": "poke", "type": 3, "description": "message command"},
                {"id": "12", "application_id": "5", "name": "stats", "type": 1, "description": "x", "options": [
                    {"type": 1, "name": "show", "description": "s", "options": [
                        {"type": 3, "name": "who", "description": "w", "required": true}]},
                    {"type": 1, "name": "reset", "description": "r"}]}
            ]
        })
    }

    #[test]
    fn the_built_ins_follow_the_account_permissions() {
        let all = built_in_commands(everything());
        assert_eq!(
            names(&all),
            [
                "ban",
                "gif",
                "kick",
                "me",
                "msg",
                "nick",
                "shrug",
                "spoiler",
                "tableflip",
                "thread",
                "timeout",
                "unflip"
            ]
        );
        let plain = built_in_commands(CommandScope {
            in_guild: true,
            ..CommandScope::default()
        });
        assert_eq!(
            names(&plain),
            [
                "gif",
                "me",
                "msg",
                "shrug",
                "spoiler",
                "tableflip",
                "unflip"
            ]
        );
        let group = built_in_commands(CommandScope {
            group_dm: true,
            ..CommandScope::default()
        });
        assert!(names(&group).contains(&"leave"));
    }

    #[test]
    fn only_chat_input_commands_of_the_index_are_listed_with_their_app() {
        let commands = index_commands(&index());
        assert_eq!(names(&commands), ["wordle", "stats"]);
        assert_eq!(commands[0].app_name, "Wordle");
        assert_eq!(commands[0].icon.as_deref(), Some("abc"));
    }

    #[test]
    fn apps_come_first_the_built_ins_last_and_prefixes_beat_substrings() {
        let mut commands = index_commands(&index());
        commands.extend(built_in_commands(everything()));
        let all = matching(&commands, "");
        assert_eq!(all.first().unwrap().name, "stats");
        assert_eq!(all.last().unwrap().name, "unflip");
        let found = matching(&commands, "me");
        assert_eq!(names(&found)[0], "me");
        assert!(names(&found).iter().position(|n| *n == "timeout") > Some(0));
        let by_app = matching(&commands, "wordl");
        assert_eq!(names(&by_app), ["wordle", "stats"]);
    }

    #[test]
    fn text_built_ins_compose_the_official_message() {
        assert_eq!(
            text_content(Builtin::Shrug, "hi").unwrap(),
            "hi ¯\\_(ツ)_/¯"
        );
        assert_eq!(text_content(Builtin::Shrug, "").unwrap(), "¯\\_(ツ)_/¯");
        assert_eq!(text_content(Builtin::Me, "waves").unwrap(), "_waves_");
        assert_eq!(text_content(Builtin::Spoiler, "x").unwrap(), "||x||");
        assert_eq!(
            text_content(Builtin::Tableflip, "").unwrap(),
            "(╯°□°)╯︵ ┻━┻"
        );
        assert!(text_content(Builtin::Kick, "x").is_none());
    }

    #[test]
    fn typed_values_become_the_option_types() {
        let integer = Opt::new("n", INTEGER, "", true);
        assert_eq!(parse_value(&integer, " 7 ").unwrap().0, json!(7));
        assert!(parse_value(&integer, "x").is_err());
        let mut text = Opt::new("s", STRING, "", true);
        text.max_length = Some(3);
        assert!(parse_value(&text, "abcd").is_err());
        assert_eq!(parse_value(&text, "abc").unwrap().0, json!("abc"));
    }

    #[test]
    fn sub_commands_are_chosen_first_and_nest_their_options() {
        let commands = index_commands(&index());
        let stats = commands[1].clone();
        let first = current_option(&stats, &[]).unwrap();
        assert_eq!(first.choices.len(), 2);
        let show = stats.options[0].clone();
        let mut filled = vec![Filled {
            opt: show,
            value: json!("show"),
            shown: "show".to_owned(),
        }];
        assert_eq!(current_option(&stats, &filled).unwrap().name, "who");
        assert!(missing_required(&stats, &filled).is_some());
        filled.push(Filled {
            opt: Opt::new("who", STRING, "", true),
            value: json!("me"),
            shown: "me".to_owned(),
        });
        assert!(missing_required(&stats, &filled).is_none());
        assert_eq!(
            nested_options(&filled),
            vec![
                json!({"type": 1, "name": "show", "options": [{"type": 3, "name": "who", "value": "me"}]})
            ]
        );
    }

    #[test]
    fn the_interaction_has_the_official_body() {
        let commands = index_commands(&index());
        let body = interaction_body(&commands[0], &[], Some(1), 2, "sess", "nonce");
        assert_eq!(body["type"], 2);
        assert_eq!(body["application_id"], "5");
        assert_eq!(body["guild_id"], "1");
        assert_eq!(body["channel_id"], "2");
        assert_eq!(body["session_id"], "sess");
        assert_eq!(body["data"]["id"], "10");
        assert_eq!(body["data"]["version"], "99");
        assert_eq!(body["data"]["type"], 1);
        assert_eq!(body["data"]["options"], json!([]));
        assert_eq!(body["data"]["application_command"]["name"], "wordle");
        assert_eq!(body["analytics_location"], "slash_ui");
    }
}
