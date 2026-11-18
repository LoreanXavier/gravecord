//! MoreCommands (Equicord `src/equicordplugins/moreCommands`, GPL-3.0-or-later): its slash commands join the built-in
//! ones while it is on. A command's `content` is sent as your message; `sendBotMessage` is Clyde's local message; an
//! error is the Commands API's "An Error occurred while executing command" from "Equicord". `uwuEveryMessage`
//! uwuifies every message you send or edit (`crate::plugins::outgoing`). Not part of the pinned client.
//!
//! GAP: `/imgtogif` always answers "No Image specified!": the app's slash commands take no file option yet
//! ("Files cannot be given to a command here yet"), so its image never arrives (the plugin's own answer without one).
//! `uwuEverything` (every text of the interface) is idle. `/systeminfo` reads the network as "4g", Chromium's
//! estimate on a desktop line, and `/gettime` writes the en-US form of `toLocaleString`.

use super::super::Root;
use super::super::slash::{
    self, BOOLEAN, Builtin, Cmd, Filled, INTEGER, NUMBER, Opt, STRING, built_in, message_option,
    string_value,
};
use dc_state::Id;
use gpui::Context;
use rand::Rng as _;
use serde_json::Value;
use std::cell::RefCell;

const ID: &str = "MoreCommands";
/// The bit of a `Builtin::Plugin` index that marks MoreCommands' commands (MessageTags' tags are `0x8000`).
pub(in crate::ui) const MORE_BIT: u16 = 0x4000;
/// The answers of the commands that fetch: `<KEY><index>:<channel>`.
pub(in crate::ui) const KEY: &str = "plugin-more-commands:";
/// `GUILD_IDS` (`@utils/constants`): Equicord's and Vencord's servers, where `/gifroulette` is restricted.
const GUILD_IDS: [Id; 2] = [1_173_279_886_065_029_291, 1_015_060_230_222_131_221];
const PRIVACY: &str = "Check your [privacy settings](<https://support.discord.com/hc/en-us/articles/21864805694999-Data-Used-to-Improve-Discord>).";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Does {
    SystemInfo,
    Uptime,
    Time,
    Choose,
    RollDice,
    FlipCoin,
    Ask,
    RandomAnimal,
    RandomNumber,
    Transform,
    WordCount,
    Countdown,
    Nekos,
    AnimeBoys,
    Ping,
    Echo,
    Lenny,
    Mock,
    Slap,
    Freaky,
    Morse,
    Uwuify,
    GifRoulette,
    FriendCloud,
    ImgToGif,
    /// The message, a space and the face.
    Kaomoji(&'static str),
}

struct Command {
    name: &'static str,
    description: &'static str,
    does: Does,
}

const fn command(name: &'static str, description: &'static str, does: Does) -> Command {
    Command {
        name,
        description,
        does,
    }
}

const fn kaomoji(name: &'static str, face: &'static str) -> Command {
    command(name, face, Does::Kaomoji(face))
}

const COMMANDS: &[Command] = &[
    command("systeminfo", "Shows system information", Does::SystemInfo),
    command("getuptime", "Returns the system uptime", Does::Uptime),
    command("gettime", "Returns the current server time", Does::Time),
    command(
        "choose",
        "Randomly chooses from provided options",
        Does::Choose,
    ),
    command(
        "rolldice",
        "Roll a die with the specified number of sides",
        Does::RollDice,
    ),
    command(
        "flipcoin",
        "Flips a coin and returns heads or tails",
        Does::FlipCoin,
    ),
    command("ask", "Ask a yes/no question and get an answer", Does::Ask),
    command(
        "randomanimal",
        "Get a random cat picture",
        Does::RandomAnimal,
    ),
    command(
        "randomnumber",
        "Generates a random number between two values",
        Does::RandomNumber,
    ),
    command(
        "transform",
        "Transform your text with the specified option",
        Does::Transform,
    ),
    command(
        "wordcount",
        "Counts the number of words in a message",
        Does::WordCount,
    ),
    command(
        "countdown",
        "Starts a countdown from a specified number",
        Does::Countdown,
    ),
    command("nekos", "Send Neko", Does::Nekos),
    command("anime-boys", "Send cute anime boys", Does::AnimeBoys),
    command(
        "ping",
        "Pings the bot to check if it's responding",
        Does::Ping,
    ),
    command("echo", "Sends a message as Clyde (locally)", Does::Echo),
    command("lenny", "Sends a lenny face", Does::Lenny),
    command("mock", "mOcK PeOpLe", Does::Mock),
    command("slap", "Slap someone/something.", Does::Slap),
    command("freaky", "it's freaky.", Does::Freaky),
    command("morse", "Translate to or from Morse code", Does::Morse),
    command("uwuify", "uwuifies your messages", Does::Uwuify),
    command(
        "gifroulette",
        "Tempt fate and send a gif",
        Does::GifRoulette,
    ),
    command(
        "friendcloud",
        "Display user you most interact with in a cloud",
        Does::FriendCloud,
    ),
    command(
        "imgtogif",
        "Allows you to turn an image to a gif",
        Does::ImgToGif,
    ),
    kaomoji("dissatisfaction", " ＞﹏＜"),
    kaomoji("smug", "ಠ_ಠ"),
    kaomoji("happy", "ヽ(´▽`)/"),
    kaomoji("crying", "ಥ_ಥ"),
    kaomoji("angry", "ヽ(｀Д´)ﾉ"),
    kaomoji("anger", "ヽ(ｏ`皿′ｏ)ﾉ"),
    kaomoji("joy", "<(￣︶￣)>"),
    kaomoji("blush", "૮ ˶ᵔ ᵕ ᵔ˶ ა"),
    kaomoji("confused", "(•ิ_•ิ)?"),
    kaomoji("sleeping", "(ᴗ_ᴗ)"),
    kaomoji("laughing", "o(≧▽≦)o"),
    kaomoji("giving", "(ﾉ◕ヮ◕)ﾉ*:･ﾟ✧"),
    kaomoji("peace", "✌(◕‿-)✌"),
    kaomoji("ending1", "Ꮺ ָ࣪ ۰ ͙⊹"),
    kaomoji("uwu", "(>⩊<)"),
    kaomoji("comfy", "(─‿‿─)♡"),
    kaomoji("lovehappy", "(*≧ω≦*)"),
    kaomoji("loveee", "(⁄ ⁄>⁄ ▽ ⁄<⁄ ⁄)"),
    kaomoji("give", "(ノ= ⩊ = )ノ"),
    kaomoji("lovegive", "ღゝ◡╹)ノ♡"),
    kaomoji("music", "(￣▽￣)/♫•¨•.¸¸♪"),
    kaomoji("stars", ".𖥔 ݁ ˖๋ ࣭ ⭑"),
    kaomoji("lovegiving", "⸜(｡˃ ᵕ ˂ )⸝♡"),
];

fn with_choices(mut option: Opt, choices: &[(&str, &str)]) -> Opt {
    option.choices = choices
        .iter()
        .map(|(label, value)| ((*label).to_owned(), Value::from(*value)))
        .collect();
    option
}

/// The command's options, as the plugin declares them (`RequiredMessageOption` is /me's, `OptionalMessageOption`
/// /shrug's).
fn options(does: Does) -> Vec<Opt> {
    let required = || message_option("Your message", true);
    let optional = || message_option("Your message", false);
    match does {
        Does::Choose => vec![Opt::new(
            "choices",
            STRING,
            "Comma-separated list of choices",
            true,
        )],
        Does::RollDice | Does::Ask | Does::WordCount | Does::Mock | Does::Uwuify => {
            vec![required()]
        }
        Does::Echo | Does::Lenny | Does::Kaomoji(_) => vec![optional()],
        Does::RandomAnimal => vec![with_choices(
            Opt::new("animal", STRING, "pick your animal", true),
            &[("cat", "cat"), ("dog", "dog")],
        )],
        Does::RandomNumber => vec![
            Opt::new("min", INTEGER, "Minimum value", true),
            Opt::new("max", INTEGER, "Maximum value", true),
        ],
        Does::Transform => vec![
            Opt::new("text", STRING, "TEXT TO UPPERCASE", true),
            with_choices(
                Opt::new(
                    "transformation",
                    STRING,
                    "transformation to apply to your text",
                    true,
                ),
                &[
                    ("toLowerCase", "toLowerCase"),
                    ("toUpperCase", "toUpperCase"),
                    ("toLocaleLowerCase", "toLocaleLowerCase"),
                    ("toLocaleUpperCase", "toLocaleUpperCase"),
                    ("stay the same", "same"),
                ],
            ),
            Opt::new("repeat", INTEGER, "how many times to repeat", false),
            Opt::new("reverse", BOOLEAN, "reverse your text", false),
            with_choices(
                Opt::new("normalize", STRING, "which normailze option to use", false),
                &[
                    ("NFC", "NFC"),
                    ("NFD", "NFD"),
                    ("NFKC", "NFKC"),
                    ("NFKD", "NFKD"),
                ],
            ),
        ],
        Does::Countdown => vec![Opt::new(
            "number",
            INTEGER,
            "Number to countdown from (max 10)",
            true,
        )],
        Does::AnimeBoys => vec![Opt::new(
            "cat",
            BOOLEAN,
            "If set, this will send exclusively cute anime cat boys",
            false,
        )],
        Does::Slap => vec![Opt::new("victim", STRING, "Thing to slap", true)],
        Does::Freaky => vec![Opt::new("message", STRING, "yoooo freaky", true)],
        Does::Morse => vec![Opt::new("text", STRING, "Text to convert", true)],
        Does::FriendCloud => vec![Opt::new(
            "count",
            NUMBER,
            "Number of users to display",
            false,
        )],
        Does::ImgToGif => vec![
            Opt::new("image", slash::ATTACHMENT, "Image attachment to use", false),
            Opt::new("width", INTEGER, "Width of the gif", false),
            Opt::new("height", INTEGER, "Height of the gif", false),
        ],
        _ => Vec::new(),
    }
}

fn value<'a>(filled: &'a [Filled], name: &str) -> Option<&'a Value> {
    filled
        .iter()
        .find(|step| step.opt.name == name)
        .map(|step| &step.value)
        .filter(|value| !value.is_null())
}

/// `findOption(opts, name, fallback)` as text: the option's value, else the fallback.
fn text_or(filled: &[Filled], name: &str, fallback: &str) -> String {
    match value(filled, name) {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => fallback.to_owned(),
    }
}

/// `parseInt`: the leading integer of the text (after white space and a sign), `None` for `NaN`.
fn parse_int(text: &str) -> Option<f64> {
    let text = text.trim_start();
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => (-1., rest),
        None => (1., text.strip_prefix('+').unwrap_or(text)),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<f64>().ok().map(|number| sign * number)
}

/// A number written as JavaScript writes it (`5`, `-2`, `NaN`).
fn js_number(number: Option<f64>) -> String {
    match number {
        Some(number) if number.fract() == 0. && number.abs() < 1e21 => format!("{number:.0}"),
        Some(number) => number.to_string(),
        None => "NaN".to_owned(),
    }
}

/// `Math.floor(Math.random() * n)`, which is `NaN` for `NaN`.
fn random_floor(n: Option<f64>, random: f64) -> Option<f64> {
    n.map(|n| (random * n).floor())
}

fn pick<T>(items: &[T], random: f64) -> Option<&T> {
    items.get((random * items.len() as f64).floor() as usize)
}

/// `mock`: every second character upper case, the others lower case.
fn mock(input: &str) -> String {
    input
        .chars()
        .enumerate()
        .map(|(index, c)| {
            if index % 2 == 1 {
                c.to_uppercase().collect::<String>()
            } else {
                c.to_lowercase().collect::<String>()
            }
        })
        .collect()
}

const FREAKY: &[(char, &str)] = &[
    ('q', "𝓺"),
    ('w', "𝔀"),
    ('e', "𝓮"),
    ('r', "𝓻"),
    ('t', "𝓽"),
    ('y', "𝔂"),
    ('u', "𝓾"),
    ('i', "𝓲"),
    ('o', "𝓸"),
    ('p', "𝓹"),
    ('a', "𝓪"),
    ('s', "𝓼"),
    ('d', "𝓭"),
    ('f', "𝓯"),
    ('g', "𝓰"),
    ('h', "𝓱"),
    ('j', "𝓳"),
    ('k', "𝓴"),
    ('l', "𝓵"),
    ('z', "𝔃"),
    ('x', "𝔁"),
    ('c', "𝓬"),
    ('v', "𝓿"),
    ('b', "𝓫"),
    ('n', "𝓷"),
    ('m', "𝓶"),
    ('Q', "𝓠"),
    ('W', "𝓦"),
    ('E', "𝓔"),
    ('R', "𝓡"),
    ('T', "𝓣"),
    ('Y', "𝓨"),
    ('U', "𝓤"),
    ('I', "𝓘"),
    ('O', "𝓞"),
    ('P', "𝓟"),
    ('A', "𝓐"),
    ('S', "𝓢"),
    ('D', "𝓓"),
    ('F', "𝓕"),
    ('G', "𝓖"),
    ('H', "𝓗"),
    ('J', "𝓙"),
    ('K', "𝓚"),
    ('L', "𝓛"),
    ('Z', "𝓩"),
    ('X', "𝓧"),
    ('C', "𝓒"),
    ('V', "𝓥"),
    ('B', "𝓑"),
    ('N', "𝓝"),
    ('M', "𝓜"),
];

/// `makeFreaky`: the script letters, "freaky" for nothing, and with `addFreakyEnding` 👅 (one time in four) or ❤️.
fn freaky(text: &str, ending: bool, random: f64) -> String {
    let trimmed = text.trim();
    let source = if trimmed.is_empty() {
        "freaky"
    } else {
        trimmed
    };
    let mut out: String = source
        .chars()
        .map(|c| {
            FREAKY
                .iter()
                .find(|(plain, _)| *plain == c)
                .map_or_else(|| c.to_string(), |(_, script)| (*script).to_owned())
        })
        .collect();
    if ending {
        out.push_str(if random < 0.25 { " 👅" } else { " ❤️" });
    }
    out
}

const MORSE: &[(char, &str)] = &[
    ('A', ".-"),
    ('B', "-..."),
    ('C', "-.-."),
    ('D', "-.."),
    ('E', "."),
    ('F', "..-."),
    ('G', "--."),
    ('H', "...."),
    ('I', ".."),
    ('J', ".---"),
    ('K', "-.-"),
    ('L', ".-.."),
    ('M', "--"),
    ('N', "-."),
    ('O', "---"),
    ('P', ".--."),
    ('Q', "--.-"),
    ('R', ".-."),
    ('S', "..."),
    ('T', "-"),
    ('U', "..-"),
    ('V', "...-"),
    ('W', ".--"),
    ('X', "-..-"),
    ('Y', "-.--"),
    ('Z', "--.."),
    ('0', "-----"),
    ('1', ".----"),
    ('2', "..---"),
    ('3', "...--"),
    ('4', "....-"),
    ('5', "....."),
    ('6', "-...."),
    ('7', "--..."),
    ('8', "---.."),
    ('9', "----."),
    (' ', "/"),
];

fn is_morse(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| matches!(c, '.' | '-' | '/' | ' '))
}

fn to_morse(text: &str) -> String {
    text.to_uppercase()
        .chars()
        .map(|c| {
            MORSE
                .iter()
                .find(|(plain, _)| *plain == c)
                .map_or("", |(_, code)| *code)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn from_morse(text: &str) -> String {
    let raw: String = text
        .split(' ')
        .map(|code| {
            MORSE
                .iter()
                .find(|(_, known)| *known == code)
                .map_or(String::new(), |(plain, _)| plain.to_string())
        })
        .collect::<String>()
        .to_lowercase();
    let mut chars = raw.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

const ENDINGS: &[&str] = &[
    "rawr x3",
    "OwO",
    "UwU",
    "o.O",
    "-.-",
    ">w<",
    "(⑅˘꒳˘)",
    "(ꈍᴗꈍ)",
    "(˘ω˘)",
    "(U ᵕ U❁)",
    "σωσ",
    "òωó",
    "(///ˬ///✿)",
    "(U ﹏ U)",
    "( ͡o ω ͡o )",
    "ʘwʘ",
    ":3",
    ":3",
    ":3",
    "XD",
    "nyaa~~",
    "mya",
    ">_<",
    "😳",
    "🥺",
    "😳😳😳",
    "rawr",
    "^^",
    "^^;;",
    "(ˆ ﻌ ˆ)♡",
    "^•ﻌ•^",
    "/(^•ω•^)",
    "(✿oωo)",
];

const REPLACEMENTS: &[(&str, &str)] = &[
    ("small", "smol"),
    ("cute", "kawaii"),
    ("fluff", "floof"),
    ("love", "luv"),
    ("stupid", "baka"),
    ("what", "nani"),
    ("meow", "nya"),
    ("hello", "hewwo"),
];

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `new RegExp("\\bword\\b", "gi")` replaced by `with`: whether it matched, and the text after.
fn replace_word(text: &str, word: &str, with: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let mut out = String::new();
    let mut last = 0;
    let mut found = false;
    let mut from = 0;
    while let Some(at) = lower[from..].find(word).map(|at| at + from) {
        let end = at + word.len();
        let before = text[..at].chars().next_back().is_some_and(is_word_char);
        let after = text[end..].chars().next().is_some_and(is_word_char);
        if !before && !after {
            out.push_str(&text[last..at]);
            out.push_str(with);
            last = end;
            found = true;
        }
        from = end;
    }
    found.then(|| {
        out.push_str(&text[last..]);
        out
    })
}

fn replace_string(word: &str) -> Option<String> {
    let mut text = word.to_owned();
    let mut replaced = false;
    for (from, to) in REPLACEMENTS {
        if let Some(next) = replace_word(&text, from, to) {
            text = next;
            replaced = true;
        }
    }
    replaced.then_some(text)
}

/// `uwuify`: the words (and the runs of space between them) one by one, and a random ending.
fn uwuify(message: &str, random: f64) -> String {
    let mut answer = String::new();
    let mut words = Vec::new();
    let mut start = 0;
    let mut space = None;
    for (index, c) in message.char_indices() {
        let is_space = c.is_whitespace();
        if space.is_some_and(|was| was != is_space) {
            words.push(&message[start..index]);
            start = index;
        }
        space = Some(is_space);
    }
    if space.is_none() {
        return String::new();
    }
    words.push(&message[start..]);
    for word in words {
        let mut chars = word.chars();
        let first = chars.next();
        if chars.all(|c| Some(c) == first) || word.starts_with("https://") {
            answer.push_str(word);
            continue;
        }
        match replace_string(word) {
            Some(replaced) => answer.push_str(&replaced),
            None => {
                let mut out = String::new();
                let chars: Vec<char> = word.chars().collect();
                for (index, c) in chars.iter().enumerate() {
                    match c {
                        'n' if matches!(chars.get(index + 1), Some('a' | 'e' | 'o')) => {
                            out.push_str("ny")
                        }
                        'l' | 'r' => out.push('w'),
                        other => out.push(*other),
                    }
                }
                answer.push_str(&out);
            }
        }
    }
    answer.push(' ');
    answer.push_str(pick(ENDINGS, random).copied().unwrap_or(":3"));
    answer
}

/// `onSend`: with `uwuEveryMessage` every message sent or edited is uwuified.
pub(crate) fn outgoing(content: String) -> String {
    let uwu = crate::plugins::with(|state| {
        state.is_on(ID) && state.value(ID, "uwuEveryMessage") == Value::Bool(true)
    });
    if uwu {
        uwuify(&content, rand::rng().random())
    } else {
        content
    }
}

/// `transform`'s text: the case, then the normal form, the reversal and the repeats.
fn transform(
    text: &str,
    how: &str,
    repeat: Option<f64>,
    reverse: bool,
    normalize: Option<&str>,
) -> Result<String, String> {
    use unicode_normalization::UnicodeNormalization as _;
    let mut text = match how {
        "toLowerCase" | "toLocaleLowerCase" => text.to_lowercase(),
        "toUpperCase" | "toLocaleUpperCase" => text.to_uppercase(),
        _ => text.to_owned(),
    };
    text = match normalize {
        Some("NFC") => text.nfc().collect(),
        Some("NFD") => text.nfd().collect(),
        Some("NFKC") => text.nfkc().collect(),
        Some("NFKD") => text.nfkd().collect(),
        _ => text,
    };
    if reverse {
        text = text.chars().rev().collect();
    }
    let count = repeat.unwrap_or(1.);
    if count < 0. || !count.is_finite() {
        return Err(format!(
            "RangeError: Invalid count value: {}",
            js_number(Some(count))
        ));
    }
    let count = count as usize;
    if text.len().saturating_mul(count) > (1 << 29) {
        return Err("RangeError: Invalid string length".to_owned());
    }
    Ok(text.repeat(count))
}

/// `"… ".split(/\s+/).length` after `trim`.
fn word_count(message: &str) -> usize {
    message.split_whitespace().count().max(1)
}

/// `Array.prototype.slice(0, count)`'s length for `count` (truncated; negative counts from the end).
fn slice_len(len: usize, count: f64) -> usize {
    let count = count.trunc();
    if count < 0. {
        len.saturating_sub((-count) as usize)
    } else {
        len.min(count as usize)
    }
}

/// `calculateAffinityScore`.
fn affinity_score(row: &Value) -> f64 {
    let number = |key: &str| row[key].as_f64().unwrap_or(0.);
    let mut score = 0.;
    if row["is_friend"] == Value::Bool(true) {
        score += 0.15 * 100.;
    }
    score += number("dm_probability") * 0.30 * 100.;
    score += number("vc_probability") * 0.25 * 100.;
    score += number("server_message_probability") * 0.20 * 100.;
    score += number("communication_probability") * 0.10 * 100.;
    (score.clamp(0., 100.) * 100.).round() / 100.
}

/// `calculateCanvasSize`.
fn canvas_size(users: usize, avatar: f64) -> (f64, f64) {
    let padding = 50.;
    let text_space = 60.;
    let item_width = avatar + padding;
    let item_height = avatar + text_space + padding;
    let cols = (users as f64 * 16. / 9.).sqrt().ceil();
    let rows = (users as f64 / cols).ceil();
    (
        (cols * item_width + padding).max(1000.),
        (rows * item_height + padding).max(700.),
    )
}

/// `generatePoissonDiskPosition`.
fn place(
    existing: &[(f64, f64, f64)],
    width: f64,
    height: f64,
    size: f64,
    random: &mut dyn FnMut() -> f64,
) -> (f64, f64) {
    let edge = 10.;
    let min_dist = size * 1.5;
    let text_space = 60.;
    let valid = |x: f64, y: f64| {
        if x < edge || x + size > width - edge || y < edge || y + size > height - text_space - edge
        {
            return false;
        }
        !existing.iter().any(|(px, py, psize)| {
            let dist = (px - x).hypot(py - y);
            dist < (psize + size) / 2. + (min_dist - size)
        })
    };
    let clamp = |x: f64, y: f64| {
        (
            x.min(width - size - edge).max(edge),
            y.min(height - size - text_space - edge).max(edge),
        )
    };
    if existing.is_empty() {
        return (width / 2. - size / 2., height / 2. - size / 2.);
    }
    for _ in 0..100 {
        let base =
            existing[((random() * existing.len() as f64).floor() as usize).min(existing.len() - 1)];
        for _ in 0..30 {
            let angle = random() * std::f64::consts::PI * 2.;
            let radius = min_dist + random() * min_dist;
            let x = base.0 + angle.cos() * radius;
            let y = base.1 + angle.sin() * radius;
            if valid(x, y) {
                return clamp(x, y);
            }
        }
    }
    for _ in 0..100 {
        let x = random() * (width - size - edge * 2.) + edge;
        let y = random() * (height - size - text_space - edge * 2.) + edge;
        if valid(x, y) {
            return clamp(x, y);
        }
    }
    (edge, edge)
}

fn total_memory_gb() -> Option<f64> {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct MemoryStatus {
            length: u32,
            load: u32,
            total_phys: u64,
            avail_phys: u64,
            total_page: u64,
            avail_page: u64,
            total_virtual: u64,
            avail_virtual: u64,
            avail_extended: u64,
        }
        unsafe extern "system" {
            fn GlobalMemoryStatusEx(status: *mut MemoryStatus) -> i32;
        }
        let mut status = MemoryStatus {
            length: std::mem::size_of::<MemoryStatus>() as u32,
            load: 0,
            total_phys: 0,
            avail_phys: 0,
            total_page: 0,
            avail_page: 0,
            total_virtual: 0,
            avail_virtual: 0,
            avail_extended: 0,
        };
        // SAFETY: the struct is MEMORYSTATUSEX's layout with its length set, as the call requires.
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) } != 0;
        ok.then(|| status.total_phys as f64 / (1u64 << 30) as f64)
    }
    #[cfg(not(windows))]
    None
}

/// `navigator.deviceMemory`: the memory in GB rounded down to a power of two, 0.25 to 8.
fn device_memory(gb: f64) -> f64 {
    let mut value = 8.;
    while value > 0.25 && value > gb {
        value /= 2.;
    }
    value
}

/// The first `chrome|firefox|safari|edge|opr` and its major version in the user agent (`/(?:…)\/?\s*(\d+)/i`).
fn browser(agent: &str) -> String {
    let lower = agent.to_ascii_lowercase();
    let mut best: Option<(usize, String)> = None;
    for name in ["chrome", "firefox", "safari", "edge", "opr"] {
        let mut from = 0;
        while let Some(at) = lower[from..].find(name).map(|at| at + from) {
            let rest = &agent[at + name.len()..];
            let after_slash = rest.strip_prefix('/').unwrap_or(rest);
            let trimmed = after_slash.trim_start();
            let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
            if !digits.is_empty() {
                let end = at + name.len() + (rest.len() - trimmed.len()) + digits.len();
                if best.as_ref().is_none_or(|(known, _)| at < *known) {
                    best = Some((at, agent[at..end].to_owned()));
                }
                break;
            }
            from = at + name.len();
        }
    }
    best.map_or_else(|| "Unknown".to_owned(), |(_, found)| found)
}

thread_local! {
    /// `/friendcloud` runs that wait for the affinities: channel and count.
    static WAITING_CLOUD: RefCell<Vec<(Id, f64)>> = const { RefCell::new(Vec::new()) };
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

impl Root {
    /// MoreCommands' commands while it is on.
    pub(in crate::ui) fn more_commands(&self) -> Vec<Cmd> {
        if !on() {
            return Vec::new();
        }
        COMMANDS
            .iter()
            .enumerate()
            .map(|(index, command)| {
                built_in(
                    -4000 - index as i32,
                    command.name,
                    Builtin::Plugin(MORE_BIT | index as u16),
                    command.description,
                    options(command.does),
                )
            })
            .collect()
    }

    fn more_error(&mut self, channel: Id, name: &str, reason: &str) {
        let reason = reason.replace("```", "\\`\\`\\`");
        self.clyde_says(
            channel,
            &format!("An Error occurred while executing command \"{name}\":\n```\n{reason}\n```"),
            "Equicord",
        );
    }

    fn more_send(&mut self, content: String) {
        self.last_typing_sent = None;
        self.send(content);
    }

    /// One of MoreCommands' commands ran (`index` without `MORE_BIT`).
    pub(in crate::ui) fn more_commands_run(
        &mut self,
        index: u16,
        filled: &[Filled],
        channel: Id,
        cx: &mut Context<Self>,
    ) {
        let Some(command) = COMMANDS.get(usize::from(index)) else {
            return;
        };
        if !on() {
            return;
        }
        let mut rng = rand::rng();
        let mut random = || rng.random::<f64>();
        match command.does {
            Does::SystemInfo => {
                let platform = if cfg!(windows) {
                    "Windows"
                } else if cfg!(target_os = "macos") {
                    "MacOS"
                } else if cfg!(target_os = "linux") {
                    "Linux"
                } else {
                    "Unknown"
                };
                let cores = std::thread::available_parallelism()
                    .map_or_else(|_| "N/A".to_owned(), |n| n.get().to_string());
                let memory = total_memory_gb().map_or_else(
                    || "N/A".to_owned(),
                    |gb| format!("{}GB", js_number(Some(device_memory(gb)))),
                );
                let (width, height) = cx
                    .primary_display()
                    .map(|display| {
                        let size = display.bounds().size;
                        (f32::from(size.width) as i64, f32::from(size.height) as i64)
                    })
                    .unwrap_or((0, 0));
                let languages = sys_locale::get_locales().collect::<Vec<_>>().join(", ");
                let info = [
                    format!("> **Platform**: {platform}"),
                    "> **Device Type**: Desktop".to_owned(),
                    format!("> **Browser**: {}", browser(dc_proto::pin::USER_AGENT)),
                    format!("> **CPU Cores**: {cores}"),
                    format!("> **Memory**: {memory}"),
                    format!("> **Screen**: {width}x{height} (24bit)"),
                    format!("> **Languages**: {languages}"),
                    "> **Network**: 4g (Online)".to_owned(),
                ]
                .join("\n");
                self.more_send(info);
            }
            Does::Uptime => {
                let minutes = crate::perf::since_process_start_ms() / 60_000;
                self.more_send(format!("> **System Uptime**: {minutes} minutes"));
            }
            Does::Time => {
                let now = chrono::Local::now();
                self.more_send(format!(
                    "> **Current Time**: {}",
                    now.format("%-m/%-d/%Y, %-I:%M:%S %p")
                ));
            }
            Does::Choose => {
                let choices: Vec<String> = text_or(filled, "choices", "")
                    .split(',')
                    .map(|choice| choice.trim().to_owned())
                    .collect();
                let choice = pick(&choices, random()).cloned().unwrap_or_default();
                self.more_send(format!("I choose: {choice}"));
            }
            Does::RollDice => {
                let sides = parse_int(&text_or(filled, "message", "6"));
                let roll = random_floor(sides, random()).map(|n| n + 1.);
                self.more_send(format!("You rolled a {}!", js_number(roll)));
            }
            Does::FlipCoin => {
                let flip = if random() < 0.5 { "Heads" } else { "Tails" };
                self.more_send(format!("The coin landed on: {flip}"));
            }
            Does::Ask => {
                let question = text_or(filled, "message", "");
                let responses = [
                    "Yes",
                    "No",
                    "Maybe",
                    "Ask again later",
                    "Definitely not",
                    "It is certain",
                ];
                let response = pick(&responses, random()).copied().unwrap_or("Yes");
                self.more_send(format!("{question} - {response}"));
            }
            Does::RandomNumber => {
                let min = parse_int(&text_or(filled, "min", "0"));
                let max = parse_int(&text_or(filled, "max", "100"));
                let number = match (min, max) {
                    (Some(min), Some(max)) => Some((random() * (max - min + 1.)).floor() + min),
                    _ => None,
                };
                self.more_send(format!(
                    "Random number between {} and {}: {}",
                    js_number(min),
                    js_number(max),
                    js_number(number)
                ));
            }
            Does::Transform => {
                let text = string_value(filled, "text");
                let how = string_value(filled, "transformation");
                let repeat = value(filled, "repeat").and_then(Value::as_f64);
                let reverse = value(filled, "reverse") == Some(&Value::Bool(true));
                let normalize = value(filled, "normalize").and_then(Value::as_str);
                match transform(&text, &how, repeat, reverse, normalize) {
                    Ok(text) => self.more_send(text),
                    Err(reason) => self.more_error(channel, command.name, &reason),
                }
            }
            Does::WordCount => {
                let count = word_count(&text_or(filled, "message", ""));
                self.clyde_says(
                    channel,
                    &format!("The message contains {count} words."),
                    "Clyde",
                );
            }
            Does::Countdown => {
                let number = parse_int(&text_or(filled, "number", "5")).map(|n| n.min(10.));
                let Some(number) = number.filter(|n| *n >= 1.) else {
                    self.clyde_says(
                        channel,
                        "Please provide a valid number between 1 and 10!",
                        "Clyde",
                    );
                    return;
                };
                let number = number as i64;
                self.clyde_says(
                    channel,
                    &format!("Starting countdown from {number}..."),
                    "Clyde",
                );
                cx.spawn(async move |this, cx| {
                    for i in (0..=number).rev() {
                        cx.background_executor()
                            .timer(std::time::Duration::from_secs(1))
                            .await;
                        let text = if i == 0 {
                            "🎉 Go! 🎉".to_owned()
                        } else {
                            format!("{i}...")
                        };
                        if this
                            .update(cx, |root, cx| {
                                root.clyde_says(channel, &text, "Clyde");
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            }
            Does::RandomAnimal | Does::Nekos | Does::AnimeBoys => {
                let url = match command.does {
                    Does::RandomAnimal => match string_value(filled, "animal").as_str() {
                        "dog" => "https://api.thedogapi.com/v1/images/search".to_owned(),
                        _ => "https://api.thecatapi.com/v1/images/search".to_owned(),
                    },
                    Does::Nekos => "https://nekos.best/api/v2/neko".to_owned(),
                    _ => {
                        let sub = if value(filled, "cat") == Some(&Value::Bool(true)) {
                            "animecatboys"
                        } else {
                            "cuteanimeboys"
                        };
                        format!("https://www.reddit.com/r/{sub}/top.json?limit=100&t=all")
                    }
                };
                let _ = self.requests.send(crate::net::Request::FetchPublicJson {
                    key: format!("{KEY}{index}:{channel}"),
                    url,
                });
            }
            Does::Ping => self.clyde_says(channel, "Pong!", "Clyde"),
            Does::Echo => {
                let content = text_or(filled, "message", "");
                self.clyde_says(channel, &content, "Clyde");
            }
            Does::Lenny => {
                let message = text_or(filled, "message", "");
                self.more_send(format!("{message} ( ͡° ͜ʖ ͡°)"));
            }
            Does::Kaomoji(face) => {
                let message = text_or(filled, "message", "");
                self.more_send(format!("{message} {face}"));
            }
            Does::Mock => {
                let message = text_or(filled, "message", "");
                self.more_send(mock(&message));
            }
            Does::Slap => {
                let victim = string_value(filled, "victim");
                let me = self.stores.current_user.as_ref().map_or(0, |me| me.id);
                self.more_send(format!(
                    "<@{me}> slaps {victim} around a bit with a large trout"
                ));
            }
            Does::Freaky => {
                let ending = crate::plugins::with(|state| {
                    state.value(ID, "addFreakyEnding") == Value::Bool(true)
                });
                let message = text_or(filled, "message", "");
                self.more_send(freaky(&message, ending, random()));
            }
            Does::Morse => {
                let input = string_value(filled, "text");
                let output = if is_morse(&input) {
                    from_morse(&input)
                } else {
                    to_morse(&input)
                };
                self.more_send(output);
            }
            Does::Uwuify => {
                let message = text_or(filled, "message", "");
                self.more_send(uwuify(&message, random()));
            }
            Does::GifRoulette => {
                if self.guild.is_some_and(|guild| GUILD_IDS.contains(&guild)) {
                    self.clyde_says(
                        channel,
                        "This command is restricted in this server.",
                        "Clyde",
                    );
                    return;
                }
                let gifs = self.favorite_gif_list();
                // `Object.keys(gifs)[random]`: none is `undefined`, sent as the text.
                let url = pick(&gifs, random())
                    .map_or_else(|| "undefined".to_owned(), |gif| gif.url.clone());
                self.more_send(url);
            }
            Does::FriendCloud => {
                let count = value(filled, "count")
                    .and_then(Value::as_f64)
                    .unwrap_or(25.);
                if count == 0. || count.is_nan() {
                    self.clyde_says(channel, "The count must be 1 or higher!", "Clyde");
                    return;
                }
                if self.active_now.affinity_rows().is_some() {
                    self.friend_cloud(channel, count);
                } else {
                    // The affinities are in the store from the start in the original; here they are fetched first.
                    WAITING_CLOUD.with(|waiting| waiting.borrow_mut().push((channel, count)));
                    let _ = self.requests.send(crate::net::Request::Fetch {
                        key: super::super::active_now::AFFINITIES.into(),
                        route: "/users/@me/affinities/v2/users".into(),
                    });
                }
            }
            Does::ImgToGif => {
                // `resolveImage` finds no upload of the `image` option (see the module's GAP).
                self.clyde_says(channel, "No Image specified!", "Clyde");
            }
        }
    }

    /// The affinities arrived: the `/friendcloud` runs that waited for them go on.
    pub(in crate::ui) fn more_commands_affinities(&mut self) {
        for (channel, count) in
            WAITING_CLOUD.with(|waiting| std::mem::take(&mut *waiting.borrow_mut()))
        {
            if on() {
                self.friend_cloud(channel, count);
            }
        }
    }

    /// `/friendcloud` with the affinities known: the people sorted by score, placed, and their avatars drawn into a
    /// picture off the UI thread, which then waits in the message box as `affinities-cloud.png`.
    fn friend_cloud(&mut self, channel: Id, count: f64) {
        let rows = self.active_now.affinity_rows().unwrap_or_default();
        if rows.is_empty() {
            self.clyde_says(channel, &format!("No affinities found. {PRIVACY}"), "Clyde");
            return;
        }
        let mut users: Vec<(Id, Option<String>, f64)> = rows
            .iter()
            .filter_map(|row| {
                let id: Id = row["other_user_id"].as_str()?.parse().ok()?;
                let user = self.stores.users.get(&id)?;
                Some((id, user.avatar.clone(), affinity_score(row)))
            })
            .collect();
        users.sort_by(|a, b| b.2.total_cmp(&a.2));
        users.truncate(slice_len(users.len(), count));
        if users.is_empty() {
            self.clyde_says(
                channel,
                &format!("No valid users found in affinities. {PRIVACY}"),
                "Clyde",
            );
            return;
        }
        let min = users.iter().map(|u| u.2).fold(f64::INFINITY, f64::min);
        let max = users.iter().map(|u| u.2).fold(f64::NEG_INFINITY, f64::max);
        let size_of = |affinity: f64| {
            if max == min {
                180.
            } else {
                120. + (affinity - min) / (max - min) * 120.
            }
        };
        let (width, height) = canvas_size(users.len(), 180.);
        let mut rng = rand::rng();
        let mut random = || rng.random::<f64>();
        let mut placed: Vec<(f64, f64, f64)> = Vec::new();
        let mut drawn = Vec::new();
        for (id, avatar, affinity) in users {
            let size = size_of(affinity);
            let (x, y) = place(&placed, width, height, size, &mut random);
            placed.push((x, y, size));
            let url = match avatar {
                Some(hash) => {
                    format!("https://cdn.discordapp.com/avatars/{id}/{hash}.webp?size=256")
                }
                // `user.member.id as number % 5` on the id's text: the number JavaScript reads from it.
                None => format!(
                    "https://cdn.discordapp.com/embed/avatars/{}.png",
                    (id as f64 % 5.) as u64
                ),
            };
            drawn.push(crate::net::CloudAvatar { url, x, y, size });
        }
        let _ = self.requests.send(crate::net::Request::FriendCloud {
            key: format!("{KEY}cloud:{channel}"),
            width: width as u32,
            height: height as u32,
            avatars: drawn,
        });
    }

    /// An answer of MoreCommands' fetches. True when the key was ours.
    pub(in crate::ui) fn more_commands_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rest) = key.strip_prefix(KEY) else {
            return false;
        };
        let Some((what, channel)) = rest.split_once(':') else {
            return true;
        };
        let Ok(channel) = channel.parse::<Id>() else {
            return true;
        };
        if !on() {
            return true;
        }
        if what == "cloud" {
            match body["path"].as_str() {
                Some(path) => {
                    // `UploadHandler.promptToUpload`: the picture waits in the channel's message box.
                    if self.channel == Some(channel) {
                        self.attach_files(vec![std::path::PathBuf::from(path)], cx);
                    }
                }
                None => self.clyde_says(channel, "Couldn't generate the image :c", "Clyde"),
            }
            return true;
        }
        let Some(command) = what
            .parse::<usize>()
            .ok()
            .and_then(|index| COMMANDS.get(index))
        else {
            return true;
        };
        match command.does {
            Does::RandomAnimal => match body[0]["url"].as_str() {
                Some(url) => self.more_send(url.to_owned()),
                None => self.clyde_says(channel, "Sorry, couldn't fetch a cat picture right now 😿", "Clyde"),
            },
            Does::Nekos => match body.get("results").and_then(|results| results.get(0)) {
                Some(result) => self.more_send(result["url"].as_str().unwrap_or_default().to_owned()),
                None => self.more_error(
                    channel,
                    command.name,
                    "TypeError: Cannot read properties of undefined (reading '0')",
                ),
            },
            Does::AnimeBoys => match body["data"]["children"].as_array() {
                Some(children) if !children.is_empty() => {
                    let index = rand::rng().random_range(0..children.len());
                    let url = children[index]["data"]["url"].as_str().unwrap_or_default();
                    self.more_send(url.to_owned());
                }
                _ => self.more_error(
                    channel,
                    command.name,
                    "TypeError: Cannot destructure property 'children' of '(intermediate value).data' as it is undefined.",
                ),
            },
            _ => {}
        }
        cx.notify();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_commands_follow_the_original() {
        assert_eq!(mock("hello"), "hElLo");
        assert_eq!(to_morse("SOS 1"), "... --- ... / .----");
        assert_eq!(from_morse("... --- ..."), "Sos");
        assert!(is_morse(".-- /") && !is_morse("hi"));
        assert_eq!(freaky("  ", false, 0.), "𝓯𝓻𝓮𝓪𝓴𝔂");
        assert_eq!(freaky("ab", true, 0.1), "𝓪𝓫 👅");
        assert_eq!(freaky("ab", true, 0.9), "𝓪𝓫 ❤️");
        assert_eq!(word_count("  "), 1);
        assert_eq!(word_count(" a  b\nc "), 3);
        assert_eq!(parse_int(" 12abc"), Some(12.));
        assert_eq!(parse_int("-3"), Some(-3.));
        assert_eq!(parse_int("abc"), None);
        assert_eq!(js_number(random_floor(None, 0.5).map(|n| n + 1.)), "NaN");
        assert_eq!(js_number(Some(5.)), "5");
    }

    #[test]
    fn uwuify_replaces_words_and_keeps_links() {
        // Ending index 0 is "rawr x3".
        assert_eq!(uwuify("hello world", 0.), "hewwo wowwd rawr x3");
        assert_eq!(uwuify("Small cats", 0.), "smol cats rawr x3");
        assert_eq!(
            uwuify("no https://example.com/lr", 0.),
            "nyo https://example.com/lr rawr x3"
        );
        assert_eq!(uwuify("aaa", 0.), "aaa rawr x3");
        assert_eq!(uwuify("", 0.), "");
        assert_eq!(uwuify("smallish", 0.), "smawwish rawr x3");
    }

    #[test]
    fn transform_applies_case_form_reversal_and_repeats() {
        assert_eq!(
            transform("Ab", "toUpperCase", Some(2.), false, None).unwrap(),
            "ABAB"
        );
        assert_eq!(transform("Ab", "same", None, true, None).unwrap(), "bA");
        assert_eq!(
            transform("ﬁ", "same", None, false, Some("NFKC")).unwrap(),
            "fi"
        );
        assert!(transform("a", "same", Some(-1.), false, None).is_err());
    }

    #[test]
    fn the_cloud_follows_the_original_numbers() {
        assert_eq!(slice_len(10, 25.), 10);
        assert_eq!(slice_len(10, 2.7), 2);
        assert_eq!(slice_len(10, -3.), 7);
        assert_eq!(canvas_size(1, 180.), (1000., 700.));
        let row = serde_json::json!({"is_friend": true, "dm_probability": 1.0});
        assert_eq!(affinity_score(&row), 45.);
        let mut random = || 0.5;
        assert_eq!(place(&[], 1000., 700., 100., &mut random), (450., 300.));
        assert_eq!(device_memory(31.9), 8.);
        assert_eq!(device_memory(3.), 2.);
        assert_eq!(browser(dc_proto::pin::USER_AGENT), "Chrome/152");
    }

    #[test]
    fn every_command_has_a_distinct_name() {
        let mut names: Vec<_> = COMMANDS.iter().map(|command| command.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COMMANDS.len());
        assert!(COMMANDS.len() < usize::from(MORE_BIT));
    }
}
