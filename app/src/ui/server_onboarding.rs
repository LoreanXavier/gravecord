//! Server Settings > Onboarding (module 989433 `ht`, `hn`, notice `hg`/`hh`, steps `hi`): the landing page (module
//! 997211), the wizard's Default Channels (`m7`, `my`, `m9`, `mM`), Questions (`g9`, `m3`, `mQ`, `mX`, `mq`, `g6`), Server
//! Guide (`gT`, `gv`, `go`, `gp`, `gb`, `gj`), Review and the edit page (`gZ`, `g1`, `gQ`, `g$`, `g0`), the Step 1 to 4
//! callouts (`hr`, `ha`, `ho`, `hd`) and the dialogs: answer (module 165394), task (251632), resource (983161), welcome sign
//! (930345), Allow @everyone (147696), the mode confirmations (`mA`) and the error alerts (module 157559). Measured in
//! `design/dom/settings/guild-onboarding-*`.
//!
//! The stores are one struct: `GuildOnboardingPromptsStore` (591552) is `Saved`, `GuildSettingsOnboardingPromptsStore`
//! (655943) is `prompts`/`errors`, `GuildSettingsDefaultChannelsStore` (863694) is `channels`,
//! `GuildSettingsOnboardingHomeSettingsStore` (132514) over `GuildOnboardingHomeSettingsStore` (701785) is `home`/`home_saved`,
//! and `GuildSettingsOnboardingStore` (509860) is `page`/`upsells_dismissed`.
//!
//! Requests: `GET`/`PUT /guilds/{id}/onboarding`, `GET`/`PUT /guilds/{id}/new-member-welcome`, `GET
//! /guilds/{id}/top-read-channels`, `PATCH /guilds/{id}/new-member-actions/{channel}`, `PUT
//! /guilds/{id}/resource-channels/{channel}`, `PUT /channels/{id}/permissions/{guild}`, `PATCH /guilds/{id}` (`home_header`).
//!
//! NOT DRAWN OR SIMPLIFIED:
//! - Preview (`X$`, module 197463): the full server preview and the member-facing onboarding flow do not exist in this
//!   client. Review's and the edit page's Preview buttons draw and do nothing; Questions' Preview link only does its
//!   official save first.
//! - Connections (`gM`, `gq`, `g5`, `gk`, `gD`, module 802941): behind experiment `2025-10_community_account_link_onboarding`,
//!   off on the fixture and absent from every capture. Saves never send `connections`.
//! - The Server Guide banner's crop dialog (module 142630): with `BANNER` the picture goes up uncropped.
//! - The shake and red flash of a blocked Back (`SHAKE_APP`, `EMPHASIZE_NOTICE`); leaving edit mode with unsaved changes
//!   drops them where the official page refuses to close.
//! - The testimonial carousel's slide, every drag animation and the drop bars (cards reorder on drop).
//! - Scrollbars of the page and of the dialogs' bodies (the shared settings scroller draws none either).
//! - Channel gating by role subscriptions (`isChannelGated`) is treated as false. Channel icons: rules, text, private
//!   text, voice, announcement and forum only; other `mh.gm` kinds draw the text icon. The single-select lists of the task
//!   and resource dialogs have no leading channel icon.
//! - The answer dialog's emoji button shows a fixed glyph, not the sprite sheet's random grey emoji.
//! - The welcome sign's author list holds only the signed-in user (the official one reads cached guild members).
//! - Context menus on channel rows (`n1.L3`) are not opened. The text areas clip rather than scroll their text.
//! - After a Next that had to save, the next step's first question is not focused.
//!
//! WRITTEN BUT NEVER RUN LIVE (no fixture data, or it would open an OS dialog or the browser):
//! - Recommended default channels (`top-read-channels` answers `[]` on the fixture), with Add, Unlock and Dismiss all.
//! - A task's thumbnail (upload, Choose Emoji, Remove Image) and `PATCH /guilds/{id}/new-member-actions/{channel}`.
//! - The banner upload and `PATCH /guilds/{id}` with `home_header`; Unlock with Boosting and the LVL 2 badge.
//! - The thirteenth-answer dropdown note, the role picker (the fixture has no roles), editing then removing one answer,
//!   reordering answers, tasks and resources, collapsing a question, the Default Channels caret, Dismiss All on suggested
//!   resources, the wizard's notice Back button, the unmount saves when leaving a step by the bar, the section list or
//!   ESC, "You have incomplete questions", the VERY_HIGH verification note, the advanced "Required + Assigns" count,
//!   See examples (it opens the browser).

use super::modal::{self, Tone};
use super::server_settings::Page;
use super::tooltip::{Side, Tip};
use super::{Root, avatar_source, color, icon};
use crate::editor::{Editor, EditorEvent, EditorStyle};
use crate::net::Request;
use crate::ui::motion::Transitions as _;
use dc_state::{Channel, ChannelKind, Guild, Id, permissions};
use gpui::{
    AnyElement, Context, Entity, EntityId, Focusable, FontWeight, HighlightStyle, IntoElement,
    SharedString, Stateful, StyledText, Subscription, Window, deferred, div, img, prelude::*, px,
    rgba,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub(super) const ONBOARDING_PREFIX: &str = "guild-onboarding:";
const GET_PREFIX: &str = "guild-onboarding:get:";
const HOME_PREFIX: &str = "guild-onboarding:home:";
const TOP_PREFIX: &str = "guild-onboarding:top:";
const SAVE_PREFIX: &str = "guild-onboarding:save:";

/// `GUILD_ONBOARDING_EXAMPLES`.
const EXAMPLES: &str = "https://support.discord.com/hc/en-us/articles/10394859532823";
const LANDING_ART: &str = "https://cdn.discordapp.com/assets/content/c986e25b68c670ea6533709a908e2a6b35a8eaaa49e3e0a0dce08a8d9bf6ba35.svg";
/// `gT`'s four, the dark theme's two (`o$.q` is false on every theme this client draws).
const GUIDE_JOINER: &str = "https://discord.com/assets/d16043e709602f96.png";
const GUIDE_MEMBER: &str = "https://discord.com/assets/47c38ca77bf70eb5.png";
const DEFAULT_PROMPT_TITLE: &str = "What do you want to do in this community?";
const WELCOME_PLACEHOLDER: &str = "Hi [@username]! I made this server so that nerds like us could have a safe space to be ourselves. I've made a bunch of lifelong friends and hope you can find the same thing too. I've curated some things for you to get started with below:";
const LANDING_WELCOME: &str = "Hi [@username]! I made this server so that nerds like us could have a safe space to be ourselves. So excited to have you here";
const SAVE_FAILED: &str = "Failed to save changes";
const GUIDE_SAVE_FAILED: &str = "Failed to save Server Guide";
const GUIDE_INVALID: &str =
    "You must have a welcome message and valid channels for any To Do tasks.";

/// `V` in module 997211: message before, emphasised, after; who said it; and the picture.
const TESTIMONIALS: [(&str, &str, &str, &str, &str); 4] = [
    (
        "Onboarding let newbies tailor their own unique experience on the server based on how they wanted to engage. It ",
        "strengthened our retention over 10%",
        " since enabling!",
        "Howl, Harry Potter admin",
        "https://cdn.discordapp.com/avatars/302407541994946561/bb3fd59e6c2ea0a86a2bdabf5dff7856.webp?size=80",
    ),
    (
        "After Design Buddies replaced our role reaction gating with Onboarding, we saw a 134% increase in retention and significantly ",
        "more new members chatting.",
        "",
        "Grace, Design Buddies founder",
        "https://cdn.discordapp.com/avatars/433499434098229249/b6d433308debef625537e33df04023e5.webp?size=80",
    ),
    (
        "We gained ",
        "~15,000 members assigning themselves a ping role",
        " in a month or so. It\u{2019}s far more effective compared to just having a self assign roles channel.",
        "cal, VALORANT admin",
        "https://cdn.discordapp.com/avatars/555460020494663691/599bfcdb0b0700b415f4c695890f272c.webp?size=80",
    ),
    (
        "Onboarding streamlines the new user experience, ",
        "breaking down customization barriers",
        ", promoting inclusivity, and delivering a personalized, welcoming experience.",
        "Dr Pigeon, Dungeon Slimes admin",
        "https://cdn.discordapp.com/guilds/936317138904440892/users/125526751064489984/avatars/f35679626acac9943e102533fc170d41.webp?size=80",
    ),
];

const MAX_PREJOIN: usize = 4;
const MAX_PROMPTS: usize = 15;
const MAX_OPTIONS: usize = 50;
const DROPDOWN_AT: usize = 13;
const MAX_TASKS: usize = 5;
const MAX_RESOURCES: usize = 7;
/// `i.message.length < 7` in `gn.Ic`, `z` and `M` of the dialogs.
const MIN_TEXT: usize = 7;
const VIEW: u64 = permissions::VIEW_CHANNEL;
const SEND: u64 = permissions::SEND_MESSAGES;
const SEND_IN_THREADS: u64 = 1 << 38;
const CONNECT: u64 = 1 << 20;
/// `PvD.VERY_HIGH`.
const VERY_HIGH: u64 = 4;
/// `ownerIcon_V0jlC3`: oklab(0.744535 0.0515917 0.151606) in `guild-onboarding-landing3`.
const CROWN: gpui::Rgba = gpui::Rgba {
    r: 0.941,
    g: 0.694,
    b: 0.196,
    a: 1.,
};

/// `uF.Hy`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum Step {
    #[default]
    Landing,
    Safety,
    Channels,
    Questions,
    Home,
    Review,
}

/// `uF.kd`.
const STEPS: [Step; 6] = [
    Step::Landing,
    Step::Safety,
    Step::Channels,
    Step::Questions,
    Step::Home,
    Step::Review,
];

/// `_k`.
fn next_step(step: Step) -> Option<Step> {
    let index = STEPS.iter().position(|known| *known == step)?;
    STEPS.get(index + 1).copied()
}

/// `kP`.
fn previous_step(step: Step) -> Option<Step> {
    let index = STEPS.iter().position(|known| *known == step)?;
    index.checked_sub(1).map(|index| STEPS[index])
}

/// `decoration` of the section: On, Off, or how far the wizard is (`uF.ti`, which skips the landing page).
fn decoration(enabled: bool, ever_enabled: bool, step: Step) -> Option<String> {
    if enabled {
        return Some("ON".to_owned());
    }
    if ever_enabled {
        return Some("OFF".to_owned());
    }
    let index = STEPS.iter().position(|known| *known == step)?;
    (index > 0).then(|| format!("{index} of {}", STEPS.len() - 1))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Emoji {
    pub(super) id: Option<String>,
    pub(super) name: Option<String>,
    animated: Option<bool>,
}

impl Emoji {
    fn from_wire(value: &Value) -> Option<Self> {
        if value.is_null() {
            return None;
        }
        Some(Self {
            id: value["id"].as_str().map(str::to_owned),
            name: value["name"].as_str().map(str::to_owned),
            animated: value["animated"].as_bool(),
        })
    }

    /// `ml.hv`: no emoji at all.
    fn is_empty(&self) -> bool {
        self.id.is_none() && self.name.is_none()
    }
}

/// `I` of module 539916, for one answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Answer {
    id: String,
    title: String,
    description: String,
    channel_ids: Vec<String>,
    role_ids: Vec<String>,
    emoji: Option<Emoji>,
}

/// `I` of module 539916.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Prompt {
    id: String,
    title: String,
    options: Vec<Answer>,
    single_select: bool,
    disabled: Option<bool>,
    required: bool,
    in_onboarding: bool,
    kind: u64,
}

impl Prompt {
    fn from_wire(value: &Value) -> Self {
        let strings = |value: &Value| -> Vec<String> {
            value
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|item| item.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        Self {
            id: value["id"].as_str().unwrap_or_default().to_owned(),
            title: value["title"].as_str().unwrap_or_default().to_owned(),
            options: value["options"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .map(|option| Answer {
                            id: option["id"].as_str().unwrap_or_default().to_owned(),
                            title: option["title"].as_str().unwrap_or_default().to_owned(),
                            description: option["description"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            channel_ids: strings(&option["channel_ids"]),
                            role_ids: strings(&option["role_ids"]),
                            emoji: Emoji::from_wire(&option["emoji"]),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            single_select: value["single_select"].as_bool().unwrap_or(false),
            disabled: value["disabled"].as_bool(),
            required: value["required"].as_bool().unwrap_or(false),
            in_onboarding: value["in_onboarding"].as_bool().unwrap_or(false),
            kind: value["type"].as_u64().unwrap_or(0),
        }
    }

    /// `SA` of module 539916.
    fn to_wire(&self) -> Value {
        json!({
            "id": self.id,
            "options": self.options.iter().map(|option| {
                let emoji = option.emoji.as_ref();
                json!({
                    "id": option.id,
                    "channel_ids": option.channel_ids,
                    "role_ids": option.role_ids,
                    "emoji": emoji.map(|emoji| json!({
                        "id": emoji.id, "name": emoji.name, "animated": emoji.animated,
                    })),
                    "emoji_id": emoji.and_then(|emoji| emoji.id.clone()),
                    "emoji_name": emoji.and_then(|emoji| emoji.name.clone()),
                    "emoji_animated": emoji.and_then(|emoji| emoji.animated),
                    "title": option.title,
                    "description": option.description,
                })
            }).collect::<Vec<_>>(),
            "title": self.title,
            "single_select": self.single_select,
            "disabled": self.disabled,
            "required": self.required,
            "in_onboarding": self.in_onboarding,
            "type": self.kind,
        })
    }

    /// `ml.Mc`: the question the Questions step starts with.
    fn starter(id: String) -> Self {
        Self {
            id,
            title: DEFAULT_PROMPT_TITLE.to_owned(),
            in_onboarding: true,
            ..Self::default()
        }
    }

    /// `ml.a5`.
    fn blank(id: String, in_onboarding: bool) -> Self {
        Self {
            id,
            in_onboarding,
            ..Self::default()
        }
    }

    /// `ml.Km`: still the untouched starter.
    fn is_starter(&self) -> bool {
        self.options.is_empty()
            && *self
                == Self {
                    id: self.id.clone(),
                    ..Self::starter(String::new())
                }
    }
}

/// `h` of module 107795.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct PromptError {
    title: Option<&'static str>,
    options: Option<&'static str>,
    config: Option<String>,
    option_errors: Vec<Option<&'static str>>,
}

const TOO_MANY_PREJOIN: &str =
    "You cannot have more than 4 questions marked as \"Ask before member joins\"";
const ROLE_CONFLICT: &str =
    "Roles used in single-select questions cannot be granted by other questions";
const NEEDS_CHANNEL_OR_ROLE: &str = "At least one channel or role is required";

/// What the store knows that validation needs (`getRole`, `getChannel`).
struct Known<'a> {
    role: &'a dyn Fn(&str) -> bool,
    channel: &'a dyn Fn(&str) -> bool,
}

/// `_` of module 107795 (`fO`).
fn answer_error(
    known: &Known,
    prompts: &[Prompt],
    prompt: &Prompt,
    answer: &Answer,
) -> Option<&'static str> {
    if prompt.single_select {
        for other in prompts.iter().filter(|other| other.id != prompt.id) {
            for option in &other.options {
                if option
                    .role_ids
                    .iter()
                    .any(|role| answer.role_ids.contains(role))
                {
                    return Some(ROLE_CONFLICT);
                }
            }
        }
    }
    let roles = answer.role_ids.iter().filter(|id| (known.role)(id)).count();
    let channels = answer
        .channel_ids
        .iter()
        .filter(|id| (known.channel)(id))
        .count();
    (roles == 0 && channels == 0).then_some(NEEDS_CHANNEL_OR_ROLE)
}

/// `h` of module 107795.
fn prompt_error(known: &Known, prompts: &[Prompt], prompt: &Prompt) -> Option<PromptError> {
    let mut error = PromptError::default();
    if prompt.title.is_empty() {
        error.title = Some("Question is required");
    }
    if prompt.options.is_empty() {
        error.options = Some("At least one answer is required");
    }
    if prompt.in_onboarding
        && prompts.iter().filter(|known| known.in_onboarding).count() > MAX_PREJOIN
    {
        error.config = Some(TOO_MANY_PREJOIN.to_owned());
    }
    error.option_errors = prompt
        .options
        .iter()
        .map(|answer| answer_error(known, prompts, prompt, answer))
        .collect();
    let bad = error.title.is_some()
        || error.options.is_some()
        || error.config.is_some()
        || error.option_errors.iter().any(Option::is_some);
    bad.then_some(error)
}

/// `xR` of module 107795: what a save sends, or why it does not.
enum SaveList {
    Nothing,
    Send(Vec<Prompt>),
    Invalid(Vec<Option<PromptError>>),
    TooMany(Vec<Option<PromptError>>),
}

fn save_list(
    known: &Known,
    saved: &[Prompt],
    edited: &[Prompt],
    ignore_default_prompt: bool,
    has_emoji: &dyn Fn(&str) -> bool,
) -> SaveList {
    if saved == edited {
        return SaveList::Nothing;
    }
    let mut changed: Vec<&str> = edited
        .iter()
        .filter(|prompt| saved.iter().find(|known| known.id == prompt.id) != Some(prompt))
        .map(|prompt| prompt.id.as_str())
        .collect();
    if ignore_default_prompt
        && changed.len() == 1
        && edited
            .iter()
            .find(|prompt| prompt.id == changed[0])
            .is_some_and(Prompt::is_starter)
    {
        changed.clear();
    }
    let cleaned: Vec<Prompt> = edited
        .iter()
        .map(|prompt| {
            if !changed.contains(&prompt.id.as_str()) {
                return prompt.clone();
            }
            let mut prompt = prompt.clone();
            for answer in &mut prompt.options {
                answer.role_ids.retain(|id| (known.role)(id));
                answer.channel_ids.retain(|id| (known.channel)(id));
                if answer
                    .emoji
                    .as_ref()
                    .and_then(|emoji| emoji.id.as_ref())
                    .is_some_and(|id| !has_emoji(id))
                {
                    answer.emoji = None;
                }
            }
            prompt.kind = u64::from(prompt.options.len() >= DROPDOWN_AT);
            prompt
        })
        .collect();
    // `xR` answers null and `dm` sends `[]`.
    if cleaned.is_empty() {
        return SaveList::Send(Vec::new());
    }
    let errors: Vec<Option<PromptError>> = cleaned
        .iter()
        .map(|prompt| {
            if changed.contains(&prompt.id.as_str()) {
                prompt_error(known, &cleaned, prompt)
            } else {
                None
            }
        })
        .collect();
    if errors.iter().any(Option::is_some) {
        return SaveList::Invalid(errors);
    }
    let (before, after): (Vec<Prompt>, Vec<Prompt>) =
        cleaned.into_iter().partition(|prompt| prompt.in_onboarding);
    if before.len() > MAX_PREJOIN {
        return SaveList::TooMany(errors);
    }
    SaveList::Send(before.into_iter().chain(after).collect())
}

/// `mH` over `c` in `m3`: one question moved, with the separator deciding which side it lands on. `None` when the
/// pre-join side would be over the limit (`Cxtq5x`).
fn move_prompt(prompts: &[Prompt], from: usize, to: usize) -> Option<Vec<Prompt>> {
    let mut items: Vec<Option<Prompt>> = prompts
        .iter()
        .filter(|prompt| prompt.in_onboarding)
        .cloned()
        .map(Some)
        .collect();
    items.push(None);
    items.extend(
        prompts
            .iter()
            .filter(|prompt| !prompt.in_onboarding)
            .cloned()
            .map(Some),
    );
    if from >= items.len() || to >= items.len() || items[from].is_none() {
        return Some(prompts.to_vec());
    }
    let moved = items.remove(from);
    items.insert(to, moved);
    let split = items.iter().position(Option::is_none)?;
    let before: Vec<Prompt> = items[..split]
        .iter()
        .flatten()
        .cloned()
        .map(|mut prompt| {
            prompt.in_onboarding = true;
            prompt
        })
        .collect();
    let after: Vec<Prompt> = items[split + 1..]
        .iter()
        .flatten()
        .cloned()
        .map(|mut prompt| {
            prompt.in_onboarding = false;
            prompt.required = false;
            prompt
        })
        .collect();
    (before.len() <= MAX_PREJOIN).then(|| before.into_iter().chain(after).collect())
}

/// `mH`: a list after one item moved.
fn moved<T: Clone>(list: &[T], from: usize, to: usize) -> Vec<T> {
    let mut list = list.to_vec();
    if from < list.len() && to < list.len() {
        let item = list.remove(from);
        list.insert(to, item);
    }
    list
}

/// `MJ` of module 488926: what @everyone has in a channel.
fn everyone_can(guild: &Guild, channel: &Channel, permission: u64) -> bool {
    let mut allowed = guild.role_permissions.get(&guild.id).copied().unwrap_or(0);
    if let Some(overwrite) = channel
        .overwrites
        .iter()
        .find(|overwrite| overwrite.id == guild.id)
    {
        allowed = (allowed & !overwrite.deny) | overwrite.allow;
    }
    allowed & permission == permission
}

fn is_vocal(channel: &Channel) -> bool {
    matches!(channel.kind, ChannelKind::Voice | ChannelKind::Other(13))
}

/// `E.tr`: the store's `SELECTABLE`.
fn is_selectable(channel: &Channel) -> bool {
    matches!(
        channel.kind,
        ChannelKind::Text | ChannelKind::Other(5 | 15 | 16)
    )
}

fn is_forum(channel: &Channel) -> bool {
    matches!(channel.kind, ChannelKind::Other(15 | 16))
}

/// `mg.S`: a default channel must be open to @everyone (gates aside).
fn can_default(guild: &Guild, channel: &Channel) -> bool {
    let needed = if is_vocal(channel) {
        VIEW | CONNECT
    } else {
        VIEW
    };
    everyone_can(guild, channel, needed)
}

/// `g` of module 961973.
fn is_chattable(guild: &Guild, channel: &Channel) -> bool {
    can_default(guild, channel)
        && if is_forum(channel) {
            everyone_can(guild, channel, SEND_IN_THREADS)
        } else {
            everyone_can(guild, channel, SEND)
        }
}

/// Module 148719.
pub(super) fn is_private(guild: &Guild, channel: &Channel) -> bool {
    if is_vocal(channel) && !everyone_can(guild, channel, CONNECT) {
        return true;
    }
    channel
        .overwrites
        .iter()
        .find(|overwrite| overwrite.id == guild.id)
        .is_some_and(|overwrite| overwrite.deny & VIEW == VIEW)
}

/// The store's list sorted by `comparator`.
fn selectable(guild: &Guild) -> Vec<&Channel> {
    let mut list: Vec<&Channel> = guild.channels.iter().filter(|c| is_selectable(c)).collect();
    list.sort_by_key(|channel| (channel.position, channel.id));
    list
}

/// `S` of module 961973 (ids only): the selectable channels a set of ids reaches, directly or by their category.
fn reached<'a>(guild: &'a Guild, ids: &[String]) -> Vec<&'a Channel> {
    selectable(guild)
        .into_iter()
        .filter(|channel| {
            can_default(guild, channel)
                && (ids.contains(&channel.id.to_string())
                    || channel
                        .parent_id
                        .is_some_and(|parent| ids.contains(&parent.to_string())))
        })
        .collect()
}

/// `tb`: the chattable ones and all of them.
fn default_reach(guild: &Guild, ids: &[String]) -> (Vec<Id>, Vec<Id>) {
    let list = reached(guild, ids);
    (
        list.iter()
            .filter(|channel| is_chattable(guild, channel))
            .map(|channel| channel.id)
            .collect(),
        list.iter().map(|channel| channel.id).collect(),
    )
}

/// `G4`: the default channels plus the fewest channels each required question can give.
fn required_reach(
    guild: &Guild,
    ids: &[String],
    prompts: &[Prompt],
    only_chattable: bool,
) -> Vec<Id> {
    let passes = |channel: &&Channel| !only_chattable || is_chattable(guild, channel);
    let mut result: Vec<Id> = reached(guild, ids)
        .into_iter()
        .filter(passes)
        .map(|channel| channel.id)
        .collect();
    for prompt in prompts.iter().filter(|prompt| prompt.required) {
        let first: Vec<Id> = reached(
            guild,
            prompt
                .options
                .first()
                .map_or(&[][..], |option| option.channel_ids.as_slice()),
        )
        .into_iter()
        .map(|channel| channel.id)
        .collect();
        let fewest = prompt.options.iter().fold(first, |fewest, option| {
            let here: Vec<Id> = reached(guild, &option.channel_ids)
                .into_iter()
                .filter(passes)
                .map(|channel| channel.id)
                .filter(|id| !result.contains(id))
                .collect();
            if here.len() < fewest.len() {
                here
            } else {
                fewest
            }
        });
        result.extend(fewest);
    }
    result
}

/// `T` of module 961973: the public channels questions and defaults reach, and those they miss.
fn coverage<'a>(
    guild: &'a Guild,
    ids: &[String],
    prompts: &[Prompt],
) -> (Vec<&'a Channel>, Vec<&'a Channel>) {
    let mut reachable: BTreeSet<String> = ids.iter().cloned().collect();
    for prompt in prompts {
        for option in &prompt.options {
            reachable.extend(option.channel_ids.iter().cloned());
        }
    }
    selectable(guild)
        .into_iter()
        .filter(|channel| !is_private(guild, channel))
        .partition(|channel| {
            reachable.contains(&channel.id.to_string())
                || channel
                    .parent_id
                    .is_some_and(|parent| reachable.contains(&parent.to_string()))
        })
}

/// `mN`: Recommended, from the top read channels.
fn recommended(guild: &Guild, top: &[String], rules: Option<Id>) -> Vec<Channel> {
    let mut announcements = 0;
    let mut list: Vec<Channel> = Vec::new();
    for id in top {
        let Some(channel) = guild.channels.iter().find(|c| c.id.to_string() == *id) else {
            continue;
        };
        if Some(channel.id) == rules {
            continue;
        }
        if channel.kind == ChannelKind::Other(5) {
            if announcements >= 2 {
                continue;
            }
            announcements += 1;
        }
        if list.len() < 5 {
            list.push(channel.clone());
        }
    }
    list.sort_by_key(|channel| !can_default(guild, channel));
    list
}

/// One group of `my`: a category row and its channels.
struct BrowserGroup<'a> {
    category: Option<&'a Channel>,
    channels: Vec<&'a Channel>,
}

/// `rz.getCategories` and `mI.zH`: Uncategorized first, categories by position, text before voice, empty groups gone.
fn browser_groups(guild: &Guild) -> Vec<BrowserGroup<'_>> {
    let mut categories: Vec<&Channel> = guild
        .channels
        .iter()
        .filter(|channel| channel.kind == ChannelKind::Category)
        .collect();
    categories.sort_by_key(|channel| (channel.position, channel.id));
    let sorted = |parent: Option<Id>| {
        let mut list: Vec<&Channel> = guild
            .channels
            .iter()
            .filter(|channel| {
                (is_selectable(channel) || is_vocal(channel)) && channel.parent_id == parent
            })
            .collect();
        list.sort_by_key(|channel| (is_vocal(channel), channel.position, channel.id));
        list
    };
    let mut groups = vec![BrowserGroup {
        category: None,
        channels: sorted(None),
    }];
    for category in categories {
        groups.push(BrowserGroup {
            category: Some(category),
            channels: sorted(Some(category.id)),
        });
    }
    groups.retain(|group| !group.channels.is_empty());
    groups
}

/// `mG.$` with `filter`: what the preview column lists.
fn sidebar_rows<'a>(guild: &'a Guild, ids: &BTreeSet<String>) -> Vec<&'a Channel> {
    let picked: Vec<&Channel> = guild
        .channels
        .iter()
        .filter(|channel| ids.contains(&channel.id.to_string()))
        .collect();
    let order = |list: &mut Vec<&'a Channel>| {
        list.sort_by_key(|channel| {
            (
                if is_vocal(channel) {
                    channel.position + 10_000
                } else {
                    channel.position
                },
                channel.id,
            )
        });
    };
    let mut categories: Vec<&Channel> = Vec::new();
    for channel in &picked {
        let id = if channel.kind == ChannelKind::Category {
            Some(channel.id)
        } else {
            channel.parent_id
        };
        if let Some(found) = id.and_then(|id| guild.channels.iter().find(|c| c.id == id))
            && !categories.iter().any(|known| known.id == found.id)
        {
            categories.push(found);
        }
    }
    categories.sort_by_key(|channel| (channel.position, channel.id));
    let mut rows: Vec<&Channel> = picked
        .iter()
        .copied()
        .filter(|channel| {
            channel.kind != ChannelKind::Category
                && !channel
                    .parent_id
                    .is_some_and(|parent| categories.iter().any(|known| known.id == parent))
        })
        .collect();
    order(&mut rows);
    for category in categories {
        rows.push(category);
        let mut children: Vec<&Channel> = if ids.contains(&category.id.to_string()) {
            guild
                .channels
                .iter()
                .filter(|channel| channel.parent_id == Some(category.id))
                .collect()
        } else {
            picked
                .iter()
                .copied()
                .filter(|channel| channel.parent_id == Some(category.id))
                .collect()
        };
        order(&mut children);
        rows.extend(children);
    }
    rows.retain(|channel| channel.kind == ChannelKind::Category || can_default(guild, channel));
    rows
}

/// `Y` in module 251632 (`NewMemberActionTypes`: 0 view, 1 chat).
fn can_complete(guild: &Guild, channel: &Channel, action: u64) -> bool {
    match action {
        0 => everyone_can(guild, channel, VIEW),
        1 if is_forum(channel) => {
            everyone_can(guild, channel, SEND_IN_THREADS) || everyone_can(guild, channel, SEND)
        }
        1 => everyone_can(guild, channel, SEND),
        _ => false,
    }
}

/// `gn.Rc`.
fn task_channel(guild: &Guild, channel: &Channel) -> bool {
    matches!(
        channel.kind,
        ChannelKind::Text | ChannelKind::Other(5 | 15 | 16)
    ) && everyone_can(guild, channel, VIEW)
}

/// `gn.Yt`.
fn resource_channel(guild: &Guild, channel: &Channel) -> bool {
    channel.kind == ChannelKind::Text
        && !everyone_can(guild, channel, SEND)
        && everyone_can(guild, channel, VIEW)
}

/// `c` of module 374084.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Task {
    pub(super) channel_id: String,
    action_type: u64,
    pub(super) title: String,
    pub(super) description: String,
    pub(super) emoji: Option<Emoji>,
    pub(super) icon: Option<String>,
}

/// `u` of module 374084.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Resource {
    pub(super) channel_id: String,
    pub(super) title: String,
    pub(super) description: String,
    pub(super) emoji: Option<Emoji>,
    pub(super) icon: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Welcome {
    pub(super) author_ids: Vec<String>,
    pub(super) message: String,
}

/// The settings object of module 374084.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Home {
    pub(super) welcome: Option<Welcome>,
    pub(super) tasks: Option<Vec<Task>>,
    pub(super) resources: Option<Vec<Resource>>,
    enabled: bool,
}

impl Home {
    /// `Xu`: channels this session cannot see are dropped.
    pub(super) fn from_wire(value: &Value, channel: &dyn Fn(&str) -> bool) -> Self {
        if value.is_null() {
            return Self::default();
        }
        let welcome = &value["welcome_message"];
        Self {
            welcome: Some(Welcome {
                author_ids: welcome["author_ids"]
                    .as_array()
                    .map(|list| {
                        list.iter()
                            .filter_map(|id| id.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                message: welcome["message"].as_str().unwrap_or_default().to_owned(),
            }),
            tasks: Some(
                value["new_member_actions"]
                    .as_array()
                    .map(|list| {
                        list.iter()
                            .filter(|item| channel(item["channel_id"].as_str().unwrap_or("")))
                            .map(|item| Task {
                                channel_id: item["channel_id"]
                                    .as_str()
                                    .unwrap_or_default()
                                    .to_owned(),
                                action_type: item["action_type"].as_u64().unwrap_or(0),
                                title: item["title"].as_str().unwrap_or_default().to_owned(),
                                description: item["description"]
                                    .as_str()
                                    .unwrap_or_default()
                                    .to_owned(),
                                emoji: Emoji::from_wire(&item["emoji"]),
                                icon: item["icon"].as_str().map(str::to_owned),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            resources: Some(
                value["resource_channels"]
                    .as_array()
                    .map(|list| {
                        list.iter()
                            .filter(|item| channel(item["channel_id"].as_str().unwrap_or("")))
                            .map(|item| Resource {
                                channel_id: item["channel_id"]
                                    .as_str()
                                    .unwrap_or_default()
                                    .to_owned(),
                                title: item["title"].as_str().unwrap_or_default().to_owned(),
                                description: item["description"]
                                    .as_str()
                                    .unwrap_or_default()
                                    .to_owned(),
                                emoji: Emoji::from_wire(&item["emoji"]),
                                icon: item["icon"].as_str().map(str::to_owned),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            enabled: value["enabled"].as_bool().unwrap_or(false),
        }
    }

    /// `Y4`.
    fn to_wire(&self, guild: Id, channel: &dyn Fn(&str) -> bool) -> Value {
        let emoji = |emoji: &Option<Emoji>| {
            let emoji = emoji.clone().unwrap_or_default();
            json!({ "id": emoji.id, "name": emoji.name, "animated": emoji.animated })
        };
        json!({
            "guild_id": guild.to_string(),
            "welcome_message": {
                "author_ids": self.welcome.as_ref().map_or(Vec::new(), |w| w.author_ids.clone()),
                "message": self.welcome.as_ref().map_or(String::new(), |w| w.message.clone()),
            },
            "new_member_actions": self.tasks.iter().flatten()
                .filter(|task| channel(&task.channel_id))
                .map(|task| json!({
                    "channel_id": task.channel_id,
                    "action_type": task.action_type,
                    "title": task.title,
                    "description": task.description,
                    "emoji": emoji(&task.emoji),
                    "icon": task.icon,
                })).collect::<Vec<_>>(),
            "resource_channels": self.resources.iter().flatten()
                .filter(|resource| channel(&resource.channel_id))
                .map(|resource| json!({
                    "channel_id": resource.channel_id,
                    "title": resource.title,
                    "description": resource.description,
                    "emoji": emoji(&resource.emoji),
                    "icon": resource.icon,
                })).collect::<Vec<_>>(),
            "enabled": self.enabled,
        })
    }

    /// `gn.jJ`.
    fn is_empty(&self) -> bool {
        welcome_is_empty(self.welcome.as_ref())
            && self.tasks.as_ref().is_none_or(Vec::is_empty)
            && self.resources.as_ref().is_none_or(Vec::is_empty)
    }

    /// `gn.Ic`.
    fn is_valid(&self, viewable: &dyn Fn(&str) -> bool) -> bool {
        if self.is_empty() {
            return true;
        }
        let Some(welcome) = self.welcome.as_ref() else {
            return false;
        };
        if welcome.message.encode_utf16().count() < MIN_TEXT || welcome.author_ids.is_empty() {
            return false;
        }
        self.tasks
            .iter()
            .flatten()
            .all(|task| viewable(&task.channel_id))
    }
}

/// `gn.Fn`.
fn welcome_is_empty(welcome: Option<&Welcome>) -> bool {
    welcome.is_none_or(|welcome| welcome.message.is_empty() && welcome.author_ids.is_empty())
}

/// `M` in module 930345.
fn welcome_error(author: bool, message: &str) -> Option<String> {
    let length = message.encode_utf16().count();
    if length == 0 {
        return None;
    }
    if !author {
        return Some(if length < MIN_TEXT {
            "Welcome messages require both an author and a message of at least 7 characters."
                .to_owned()
        } else {
            "Welcome messages require an author".to_owned()
        });
    }
    (length < MIN_TEXT)
        .then(|| "Welcome messages require a message of at least 7 characters.".to_owned())
}

/// `z` in module 251632: the title's error (the dialog shows it once anything is typed).
fn task_title_error(title: &str) -> Option<&'static str> {
    (title.encode_utf16().count() < MIN_TEXT).then_some("Actions must be at least 7 characters.")
}

/// `new nD.LG(error).getAnyErrorMessageAndField()`, joined as the dialogs join it.
fn field_error(body: &Value) -> String {
    fn walk(value: &Value, field: Option<&str>) -> Option<(Option<String>, String)> {
        match value {
            Value::Object(map) => {
                if let Some(errors) = map.get("_errors").and_then(Value::as_array) {
                    return errors.first().and_then(|first| {
                        first["message"]
                            .as_str()
                            .map(|message| (field.map(str::to_owned), message.to_owned()))
                    });
                }
                map.iter()
                    .filter(|(key, _)| *key != "message" && *key != "code")
                    .find_map(|(key, value)| walk(value, Some(key)))
            }
            Value::Array(list) => list.iter().find_map(|value| walk(value, field)),
            Value::String(text) => Some((field.map(str::to_owned), text.clone())),
            _ => None,
        }
    }
    if let Some(errors) = body.get("errors")
        && let Some((field, message)) = walk(errors, None)
    {
        return field.map_or(message.clone(), |field| format!("{field}: {message}"));
    }
    if let Some((field, message)) = walk(body, None) {
        return field.map_or(message.clone(), |field| format!("{field}: {message}"));
    }
    body["message"].as_str().unwrap_or_default().to_owned()
}

/// `E().flatMap(e.body, t => t)` in module 199940.
fn guide_error(body: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(map) = body.as_object() {
        for value in map.values() {
            match value {
                Value::Array(list) => parts.extend(list.iter().map(|item| match item.as_str() {
                    Some(text) => text.to_owned(),
                    None => item.to_string(),
                })),
                Value::String(text) => parts.push(text.clone()),
                other => parts.push(other.to_string()),
            }
        }
    }
    parts.join(", ")
}

/// `CHANNEL_PERMISSIONS_OVERWRITE`'s body in `R` of module 495273 with `n` true: the access permissions neither denied nor
/// allowed for @everyone any more.
fn unlocked_overwrite(guild: Id, channel: &Channel) -> Value {
    let access = if is_vocal(channel) {
        VIEW | CONNECT
    } else {
        VIEW
    };
    let found = channel
        .overwrites
        .iter()
        .find(|overwrite| overwrite.id == guild);
    json!({
        "id": guild.to_string(),
        "type": 0,
        "allow": (found.map_or(0, |overwrite| overwrite.allow) & !access).to_string(),
        "deny": (found.map_or(0, |overwrite| overwrite.deny) & !access).to_string(),
    })
}

/// `ml.Uh`, the parts this page reads.
#[derive(Clone, Debug, Default, PartialEq)]
struct Saved {
    prompts: Vec<Prompt>,
    default_channels: Vec<String>,
    enabled: bool,
    mode: u64,
}

impl Saved {
    fn from_wire(body: &Value, guild: &Guild) -> Self {
        Self {
            prompts: body["prompts"]
                .as_array()
                .map(|list| list.iter().map(Prompt::from_wire).collect())
                .unwrap_or_default(),
            // `defaultChannelIds: i.filter(e => (0, u.S)(t, e))`.
            default_channels: body["default_channel_ids"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .filter(|id| {
                            guild
                                .channels
                                .iter()
                                .find(|channel| channel.id.to_string() == *id)
                                .is_some_and(|channel| {
                                    channel.kind == ChannelKind::Category
                                        || can_default(guild, channel)
                                })
                        })
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            enabled: body["enabled"].as_bool().unwrap_or(false),
            mode: body["mode"].as_u64().unwrap_or(0),
        }
    }
}

/// A text field and its listener.
pub(super) struct Field {
    editor: Entity<Editor>,
    _events: Subscription,
}

impl Field {
    fn text(&self, cx: &gpui::App) -> String {
        self.editor.read(cx).text().to_owned()
    }
}

/// Which dropdown of a dialog is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dropdown {
    Channels,
    Roles,
    Single,
    ThumbMenu,
}

/// Module 165394.
pub(super) struct AnswerDialog {
    prompt: String,
    answer: Option<String>,
    index: usize,
    prompt_title: String,
    title: Field,
    description: Field,
    channels: Vec<String>,
    roles: Vec<String>,
    channel_query: Field,
    role_query: Field,
    emoji: Option<Emoji>,
    title_error: Option<&'static str>,
    roles_error: Option<&'static str>,
}

/// Module 251632.
pub(super) struct TaskDialog {
    editing: Option<String>,
    title: Field,
    channel: Option<String>,
    action: Option<u64>,
    emoji: Option<Emoji>,
    icon: Option<String>,
    icon_key: Option<String>,
    first_emoji: Option<Emoji>,
    first_icon: Option<String>,
    action_error: Option<&'static str>,
}

/// Module 983161.
pub(super) struct ResourceDialog {
    editing: Option<String>,
    channel: Option<String>,
    title: Field,
    description: Field,
    icon: Option<String>,
}

/// Module 930345.
pub(super) struct WelcomeDialog {
    author: Option<String>,
    message: Field,
}

/// Module 147696: the note first, then the channels.
pub(super) struct UnlockDialog {
    reviewing: bool,
    channels: Vec<Id>,
    selected: Vec<Id>,
}

pub(super) enum Dialog {
    Answer(Box<AnswerDialog>),
    Task(Box<TaskDialog>),
    Resource(Box<ResourceDialog>),
    Welcome(Box<WelcomeDialog>),
    Unlock(UnlockDialog),
}

/// `mA`'s confirmations and module 157559's alerts.
pub(super) enum Confirm {
    Mode {
        advanced: bool,
    },
    Alert {
        title: String,
        body: String,
    },
    /// `mq`: the thirteenth answer warns first.
    Dropdown {
        prompt: String,
        index: usize,
    },
}

/// What a finished save goes on to do.
#[derive(Clone, Debug, PartialEq, Eq)]
enum After {
    Nothing,
    Go(Step),
    /// `gZ`'s `s`: `(await gr(t, !0), e && UP(t, !0))`.
    EnableGuide,
    /// `W5(t, l).then(() => E0(t, channel, icon))`: the picture only after the guide is saved, or the two race.
    ResourceIcon {
        channel: String,
        uri: String,
    },
}

/// What a request in flight was.
#[derive(Clone, Debug, PartialEq)]
enum Sent {
    Prompts(Vec<Prompt>),
    Channels {
        channels: Vec<String>,
        prompts: Option<Vec<Prompt>>,
    },
    Enabled(bool),
    Mode(u64),
    Guide(Home),
    TaskIcon(String),
    ResourceIcon {
        channel: String,
        settings: Option<Home>,
    },
    Unlock(Id),
    HomeHeader,
}

pub(super) struct OnboardingState {
    guild: Id,
    loaded: bool,
    saved: Saved,
    prompts: Vec<Prompt>,
    errors: Vec<Option<PromptError>>,
    submitting: bool,
    advanced: bool,
    channels: BTreeSet<String>,
    channels_collapsed: bool,
    recommended_dismissed: bool,
    top_channels: Vec<String>,
    home_saved: Home,
    home: Home,
    /// `N` in module 132514: pictures waiting for their task to exist (emoji, or a data URI).
    pending_icons: HashMap<String, (Option<String>, Option<Emoji>)>,
    /// Persisted officially (`dismissedSuggestedChannelIdsByGuildId`); this session only here.
    dismissed_suggestions: Vec<String>,
    step: Step,
    upsells_dismissed: BTreeSet<&'static str>,
    testimonial: usize,
    collapsed: BTreeSet<String>,
    seeded_starter: bool,
    coverage_open: bool,
    examples_shown: BTreeSet<String>,
    guide_tab_member: bool,
    home_header: Option<String>,
    verification: u64,
    fields: BTreeMap<String, Field>,
    focused: Option<EntityId>,
    dialog: Option<Dialog>,
    dropdown: Option<Dropdown>,
    confirm: Option<Confirm>,
    sent: HashMap<String, (Sent, After)>,
    sequence: u64,
}

impl OnboardingState {
    fn new(guild: Id) -> Self {
        Self {
            guild,
            loaded: false,
            saved: Saved::default(),
            prompts: Vec::new(),
            errors: Vec::new(),
            submitting: false,
            advanced: false,
            channels: BTreeSet::new(),
            channels_collapsed: false,
            recommended_dismissed: false,
            top_channels: Vec::new(),
            home_saved: Home::default(),
            home: Home::default(),
            pending_icons: HashMap::new(),
            dismissed_suggestions: Vec::new(),
            step: Step::Landing,
            upsells_dismissed: BTreeSet::new(),
            testimonial: 0,
            collapsed: BTreeSet::new(),
            seeded_starter: false,
            coverage_open: false,
            examples_shown: BTreeSet::new(),
            guide_tab_member: false,
            home_header: None,
            verification: 0,
            fields: BTreeMap::new(),
            focused: None,
            dialog: None,
            dropdown: None,
            confirm: None,
            sent: HashMap::new(),
            sequence: 0,
        }
    }

    fn channels_changed(&self) -> bool {
        let saved: BTreeSet<String> = self.saved.default_channels.iter().cloned().collect();
        saved != self.channels
    }

    fn prompts_changed(&self) -> bool {
        self.saved.prompts != self.prompts
    }

    fn home_changed(&self) -> bool {
        self.home != self.home_saved
    }

    /// `hasChanges` of module 509860.
    fn step_changed(&self) -> bool {
        match self.step {
            Step::Channels if self.saved.mode == 1 => {
                self.channels_changed() || self.prompts_changed()
            }
            Step::Channels => self.channels_changed(),
            Step::Questions => self.prompts_changed(),
            Step::Home => self.home_changed(),
            _ => false,
        }
    }

    /// `hasConfiguredAnythingForCurrentStep`.
    fn step_configured(&self) -> bool {
        match self.step {
            Step::Safety => true,
            Step::Channels => !self.channels.is_empty(),
            Step::Questions => !self.prompts.is_empty(),
            Step::Home => !self.home.is_empty(),
            _ => false,
        }
    }

    /// `hasErrors`.
    fn has_errors(&self) -> bool {
        self.step == Step::Questions && self.errors.iter().any(Option::is_some)
    }

    /// `ql` and `isEducationUpsellDismissed`.
    fn upsell(&self) -> Option<&'static str> {
        let name = match self.step {
            Step::Safety => "Safety Check",
            Step::Channels => "Default Channels",
            Step::Questions => "Customization Questions",
            Step::Home => "Home Settings",
            _ => return None,
        };
        (!self.upsells_dismissed.contains(name)).then_some(name)
    }

    fn next_key(&mut self) -> String {
        self.sequence += 1;
        format!("{SAVE_PREFIX}{}:{}", self.guild, self.sequence)
    }

    /// The fields open now, for typing in tests.
    #[cfg(feature = "capture")]
    pub(super) fn editors(&self) -> Vec<Entity<Editor>> {
        let mut list: Vec<Entity<Editor>> = self
            .fields
            .values()
            .map(|field| field.editor.clone())
            .collect();
        match self.dialog.as_ref() {
            Some(Dialog::Answer(dialog)) => {
                list.push(dialog.title.editor.clone());
                list.push(dialog.description.editor.clone());
                list.push(dialog.channel_query.editor.clone());
                list.push(dialog.role_query.editor.clone());
            }
            Some(Dialog::Task(dialog)) => list.push(dialog.title.editor.clone()),
            Some(Dialog::Resource(dialog)) => {
                list.push(dialog.title.editor.clone());
                list.push(dialog.description.editor.clone());
            }
            Some(Dialog::Welcome(dialog)) => list.push(dialog.message.editor.clone()),
            _ => {}
        }
        list
    }
}

/// The feature `uB` reads.
fn ever_enabled(guild: Option<&Guild>) -> bool {
    guild.is_some_and(|guild| {
        guild
            .features
            .iter()
            .any(|feature| feature == "GUILD_ONBOARDING_EVER_ENABLED")
    })
}

fn has_feature(guild: Option<&Guild>, name: &str) -> bool {
    guild.is_some_and(|guild| guild.features.iter().any(|feature| feature == name))
}

/// `String(Date.now())`, kept from repeating.
fn local_id(sequence: u64) -> String {
    (chrono::Utc::now().timestamp_millis() as u64 + sequence).to_string()
}

fn field_style(size: f32, line_height: f32) -> EditorStyle {
    let mut style = super::editor_style();
    style.size = size;
    style.line_height = line_height;
    style.placeholder = super::hsla("--input-placeholder-text-default");
    style
}

/// `maxLength`.
fn clipped(text: &str, limit: usize) -> Option<String> {
    (text.encode_utf16().count() > limit).then(|| {
        let mut length = 0;
        text.chars()
            .take_while(|letter| {
                length += letter.len_utf16();
                length <= limit
            })
            .collect()
    })
}

impl Root {
    fn onboarding(&self) -> Option<&OnboardingState> {
        self.server_settings.as_ref()?.onboarding.as_ref()
    }

    fn onboarding_mut(&mut self) -> Option<&mut OnboardingState> {
        self.server_settings.as_mut()?.onboarding.as_mut()
    }

    /// The page's reads. Server settings ask once per open guild, for the sidebar's decoration too.
    pub(super) fn request_onboarding(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        if open
            .onboarding
            .as_ref()
            .is_some_and(|state| state.guild == guild)
        {
            return;
        }
        open.onboarding = Some(OnboardingState::new(guild));
        for (prefix, route) in [
            (GET_PREFIX, format!("/guilds/{guild}/onboarding")),
            (HOME_PREFIX, format!("/guilds/{guild}/new-member-welcome")),
            (TOP_PREFIX, format!("/guilds/{guild}/top-read-channels")),
        ] {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{prefix}{guild}")),
                method: reqwest::Method::GET,
                route,
                body: None,
            });
        }
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{ONBOARDING_PREFIX}guild:{guild}")),
            method: reqwest::Method::GET,
            route: format!("/guilds/{guild}"),
            body: None,
        });
        let _ = cx;
    }

    /// Opening the page itself: its pictures, and Safety Check's data.
    pub(super) fn request_onboarding_page(&mut self, guild: Id, cx: &mut Context<Self>) {
        self.request_onboarding(guild, cx);
        for url in [LANDING_ART, GUIDE_JOINER, GUIDE_MEMBER] {
            self.request_image(url.to_owned(), url.to_owned());
        }
        for (_, _, _, _, avatar) in TESTIMONIALS {
            self.request_image(avatar.to_owned(), avatar.to_owned());
        }
        for emoji in ["\u{1F9D9}", "\u{1F3A4}", "\u{1F440}"] {
            self.request_emoji_image(emoji);
        }
        if let Some(user) = self.stores.current_user.clone() {
            let (key, url) = avatar_source(&user);
            self.request_image(key, url);
        }
        self.request_community_safety(guild, cx);
        self.onboarding_request_pictures();
    }

    fn onboarding_channel_known(&self, guild: Id) -> impl Fn(&str) -> bool + use<> {
        let ids: BTreeSet<String> = self
            .stores
            .guild(guild)
            .map(|known| known.channels.iter().map(|c| c.id.to_string()).collect())
            .unwrap_or_default();
        move |id: &str| ids.contains(id)
    }

    fn onboarding_role_known(&self, guild: Id) -> impl Fn(&str) -> bool + use<> {
        let ids: BTreeSet<String> = self
            .stores
            .guild(guild)
            .map(|known| known.roles.keys().map(ToString::to_string).collect())
            .unwrap_or_default();
        move |id: &str| ids.contains(id)
    }

    fn onboarding_viewable(&self, guild: Id) -> impl Fn(&str) -> bool + use<> {
        let ids: BTreeSet<String> = self
            .stores
            .guild(guild)
            .map(|known| {
                known
                    .channels
                    .iter()
                    .filter(|channel| everyone_can(known, channel, VIEW))
                    .map(|channel| channel.id.to_string())
                    .collect()
            })
            .unwrap_or_default();
        move |id: &str| ids.contains(id)
    }

    /// Every picture the page's data names.
    fn onboarding_request_pictures(&mut self) {
        let Some(state) = self.onboarding() else {
            return;
        };
        let mut emojis: Vec<Emoji> = Vec::new();
        for prompt in &state.prompts {
            emojis.extend(prompt.options.iter().filter_map(|a| a.emoji.clone()));
        }
        for task in state.home.tasks.iter().flatten() {
            emojis.extend(task.emoji.clone());
        }
        let mut pictures: Vec<String> = Vec::new();
        for task in state.home.tasks.iter().flatten() {
            pictures.extend(Self::task_icon_url(task));
        }
        for resource in state.home.resources.iter().flatten() {
            pictures.extend(Self::resource_icon_url(resource));
        }
        let authors: Vec<String> = state
            .home
            .welcome
            .iter()
            .flat_map(|welcome| welcome.author_ids.clone())
            .collect();
        for emoji in emojis {
            self.onboarding_request_emoji(&emoji);
        }
        for url in pictures {
            self.request_image(url.clone(), url);
        }
        for author in authors {
            let user = author
                .parse::<Id>()
                .ok()
                .and_then(|id| self.stores.users.get(&id).cloned());
            if let Some(user) = user {
                let (key, url) = avatar_source(&user);
                self.request_image(key, url);
            }
        }
    }

    /// `getNewMemberActionIconURL`.
    fn task_icon_url(task: &Task) -> Option<String> {
        task.icon.as_ref().map(|icon| {
            format!(
                "https://cdn.discordapp.com/new-member-actions/{}/{icon}.png?size=128",
                task.channel_id
            )
        })
    }

    /// `getResourceChannelIconURL`.
    fn resource_icon_url(resource: &Resource) -> Option<String> {
        resource.icon.as_ref().map(|icon| {
            format!(
                "https://cdn.discordapp.com/resource-channels/{}/{icon}.png?size=128",
                resource.channel_id
            )
        })
    }

    fn onboarding_request_emoji(&mut self, emoji: &Emoji) {
        if let Some(id) = emoji.id.as_ref() {
            self.request_image(
                format!("custom:{id}"),
                format!("https://cdn.discordapp.com/emojis/{id}.webp?size=48"),
            );
        } else if let Some(name) = emoji.name.as_ref() {
            self.request_emoji_image(name);
        }
    }

    fn onboarding_emoji_image(&self, emoji: &Emoji) -> Option<std::sync::Arc<gpui::Image>> {
        if let Some(id) = emoji.id.as_ref() {
            return self.images.get(&format!("custom:{id}")).cloned();
        }
        self.images
            .get(&format!("emoji:{}", emoji.name.as_ref()?))
            .cloned()
    }

    pub(super) fn onboarding_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let report = self.fetched.get(key).cloned().unwrap_or_default();
        let guild = self.onboarding().map_or(0, |state| state.guild);
        if key.starts_with(GET_PREFIX) {
            let known = self.stores.guild(guild).cloned();
            if let (Some(known), true) = (known, report["ok"] == true) {
                let saved = Saved::from_wire(&report["body"], &known);
                if let Some(state) = self.onboarding_mut() {
                    state.prompts = saved.prompts.clone();
                    state.channels = saved.default_channels.iter().cloned().collect();
                    state.advanced = saved.mode == 1;
                    state.saved = saved;
                    state.errors.clear();
                }
            }
            if let Some(state) = self.onboarding_mut() {
                state.loaded = true;
            }
            self.onboarding_seed_starter();
            self.onboarding_sync_fields(cx);
        } else if key.starts_with(HOME_PREFIX) {
            let channel = self.onboarding_channel_known(guild);
            let home = Home::from_wire(&report["body"], &channel);
            if let Some(state) = self.onboarding_mut() {
                state.home_saved = home.clone();
                state.home = home;
            }
        } else if key.starts_with(TOP_PREFIX) {
            if let Some(state) = self.onboarding_mut() {
                state.top_channels = report["body"]
                    .as_array()
                    .map(|list| {
                        list.iter()
                            .filter_map(|id| id.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
            }
        } else if key.starts_with(SAVE_PREFIX) {
            self.onboarding_answered(key, &report, cx);
        } else if let Some(state) = self.onboarding_mut() {
            state.verification = report["body"]["verification_level"].as_u64().unwrap_or(0);
            state.home_header = report["body"]["home_header"].as_str().map(str::to_owned);
        }
        self.onboarding_request_pictures();
        cx.notify();
    }

    fn onboarding_alert(&mut self, title: &str, body: String) {
        if let Some(state) = self.onboarding_mut() {
            state.confirm = Some(Confirm::Alert {
                title: title.to_owned(),
                body,
            });
        }
    }

    fn onboarding_run_after(
        &mut self,
        after: After,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        match after {
            After::Nothing => {}
            After::Go(step) => self.onboarding_enter(step, window, cx),
            After::EnableGuide => self.onboarding_set_guide_enabled(true, cx),
            After::ResourceIcon { channel, uri } => {
                let guild = self.onboarding().map_or(0, |state| state.guild);
                self.onboarding_send(
                    format!("/guilds/{guild}/resource-channels/{channel}"),
                    reqwest::Method::PUT,
                    json!({ "icon": uri }),
                    Sent::ResourceIcon {
                        channel,
                        settings: None,
                    },
                    After::Nothing,
                );
            }
        }
    }

    /// A request's answer: the `try` around each of modules 107795, 199940 and 923121's sends.
    fn onboarding_answered(&mut self, key: &str, report: &Value, cx: &mut Context<Self>) {
        let Some((sent, after)) = self
            .onboarding_mut()
            .and_then(|state| state.sent.remove(key))
        else {
            return;
        };
        let ok = report["ok"] == true;
        let body = &report["body"];
        let guild = self.onboarding().map_or(0, |state| state.guild);
        match sent {
            Sent::Prompts(list) => {
                if let Some(state) = self.onboarding_mut() {
                    state.submitting = false;
                    if ok {
                        // `GUILD_SETTINGS_ONBOARDING_PROMPTS_SAVE_SUCCESS`: what was sent, not what came back.
                        state.saved.prompts = list.clone();
                        state.prompts = list;
                        state.errors.clear();
                    }
                }
                if ok {
                    self.onboarding_sync_fields(cx);
                    self.onboarding_run_after(after, None, cx);
                } else {
                    self.onboarding_alert(SAVE_FAILED, field_error(body));
                }
            }
            Sent::Channels { channels, prompts } => {
                if ok {
                    if let Some(state) = self.onboarding_mut() {
                        state.saved.default_channels = channels.clone();
                        state.channels = channels.into_iter().collect();
                        if let Some(list) = prompts {
                            state.saved.prompts = list.clone();
                            state.prompts = list;
                            state.errors.clear();
                        }
                    }
                    self.onboarding_sync_fields(cx);
                } else {
                    self.onboarding_alert(SAVE_FAILED, field_error(body));
                }
                // `ma` swallows its failure, and `f` goes on.
                let advanced = self.onboarding().is_some_and(|state| state.advanced);
                if advanced {
                    self.onboarding_save_prompts(true, after, cx);
                } else {
                    self.onboarding_run_after(after, None, cx);
                }
            }
            Sent::Enabled(_) => {
                if !ok {
                    self.onboarding_alert(SAVE_FAILED, field_error(body));
                }
                self.onboarding_run_after(after, None, cx);
            }
            Sent::Mode(mode) => {
                if !ok {
                    self.onboarding_alert(SAVE_FAILED, field_error(body));
                }
                // `CP` dispatches the mode whether or not the save worked.
                if let Some(state) = self.onboarding_mut() {
                    state.saved.mode = mode;
                    state.advanced = mode == 1;
                }
            }
            Sent::Guide(sent_home) => {
                let channel = self.onboarding_channel_known(guild);
                if ok {
                    let home = Home::from_wire(body, &channel);
                    let mut uploads: Vec<(String, String)> = Vec::new();
                    if let Some(state) = self.onboarding_mut() {
                        state.home_saved = home.clone();
                        state.home = home;
                        for (channel, (icon, emoji)) in state.pending_icons.drain() {
                            if emoji.is_none()
                                && let Some(icon) = icon
                            {
                                uploads.push((channel, icon));
                            }
                        }
                    }
                    for (channel, icon) in uploads {
                        self.onboarding_send_task_icon(guild, &channel, json!({ "icon": icon }));
                    }
                    self.onboarding_request_pictures();
                } else {
                    if let Some(state) = self.onboarding_mut() {
                        // `GUILD_HOME_SETTINGS_UPDATE_FAIL`: the edits go, with the pictures waiting.
                        state.pending_icons.clear();
                        state.home = state.home_saved.clone();
                    }
                    self.onboarding_alert(
                        GUIDE_SAVE_FAILED,
                        format!(
                            "Check that the permissions on your To Do and Resource channels are set correctly.\n\nErrors: {}",
                            guide_error(body)
                        ),
                    );
                }
                let _ = sent_home;
                // `.then`: a failed save stops the picture too.
                if ok || !matches!(after, After::ResourceIcon { .. }) {
                    self.onboarding_run_after(after, None, cx);
                }
            }
            Sent::TaskIcon(channel) => {
                if ok {
                    let icon = body["icon"].as_str().map(str::to_owned);
                    let emoji = Emoji::from_wire(&body["emoji"]);
                    if let Some(state) = self.onboarding_mut() {
                        for home in [&mut state.home, &mut state.home_saved] {
                            if let Some(task) = home
                                .tasks
                                .iter_mut()
                                .flatten()
                                .find(|task| task.channel_id == channel)
                            {
                                task.icon = icon.clone();
                                task.emoji = emoji.clone();
                            }
                        }
                    }
                    self.onboarding_request_pictures();
                } else {
                    self.onboarding_alert(
                        GUIDE_SAVE_FAILED,
                        format!(
                            "Check that the permissions on your To Do and Resource channels are set correctly.\n\nErrors: {}",
                            guide_error(body)
                        ),
                    );
                }
            }
            Sent::ResourceIcon { channel, settings } => {
                let _ = settings;
                if ok {
                    let icon = body["icon"].as_str().map(str::to_owned);
                    if let Some(state) = self.onboarding_mut() {
                        for home in [&mut state.home, &mut state.home_saved] {
                            if let Some(resource) = home
                                .resources
                                .iter_mut()
                                .flatten()
                                .find(|resource| resource.channel_id == channel)
                            {
                                resource.icon = icon.clone();
                            }
                        }
                    }
                    self.onboarding_request_pictures();
                } else {
                    self.onboarding_alert(
                        GUIDE_SAVE_FAILED,
                        format!(
                            "Check that the permissions on your To Do and Resource channels are set correctly.\n\nErrors: {}",
                            guide_error(body)
                        ),
                    );
                }
            }
            Sent::Unlock(_) => {}
            Sent::HomeHeader => {
                if ok {
                    if let Some(state) = self.onboarding_mut() {
                        state.home_header = body["home_header"].as_str().map(str::to_owned);
                    }
                } else {
                    self.onboarding_alert(GUIDE_SAVE_FAILED, field_error(body));
                }
            }
        }
    }

    fn onboarding_send(
        &mut self,
        route: String,
        method: reqwest::Method,
        body: Value,
        sent: Sent,
        after: After,
    ) {
        let Some(state) = self.onboarding_mut() else {
            return;
        };
        let key = state.next_key();
        state.sent.insert(key.clone(), (sent, after));
        let _ = self.requests.send(Request::Rest {
            key: Some(key),
            method,
            route,
            body: Some(body),
        });
    }

    /// `dm`.
    fn onboarding_save_prompts(
        &mut self,
        ignore_default: bool,
        after: After,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.onboarding() else {
            return;
        };
        if !state.prompts_changed() {
            self.onboarding_run_after(after, None, cx);
            return;
        }
        let guild = state.guild;
        let role = self.onboarding_role_known(guild);
        let channel = self.onboarding_channel_known(guild);
        let known = self.stores.guild(guild);
        let has_emoji = |id: &str| {
            known.is_some_and(|known| known.emojis.iter().any(|emoji| emoji.id.to_string() == id))
        };
        let list = save_list(
            &Known {
                role: &role,
                channel: &channel,
            },
            &state.saved.prompts,
            &state.prompts,
            ignore_default,
            &has_emoji,
        );
        match list {
            SaveList::Nothing => self.onboarding_run_after(after, None, cx),
            SaveList::Invalid(errors) => {
                if let Some(state) = self.onboarding_mut() {
                    state.errors = errors;
                    state.submitting = false;
                }
            }
            SaveList::TooMany(errors) => {
                if let Some(state) = self.onboarding_mut() {
                    state.errors = errors;
                    state.submitting = false;
                }
                self.onboarding_alert(SAVE_FAILED, TOO_MANY_PREJOIN.to_owned());
            }
            SaveList::Send(list) => {
                if let Some(state) = self.onboarding_mut() {
                    state.submitting = true;
                    state.errors.clear();
                }
                let body =
                    json!({ "prompts": list.iter().map(Prompt::to_wire).collect::<Vec<_>>() });
                self.onboarding_send(
                    format!("/guilds/{guild}/onboarding"),
                    reqwest::Method::PUT,
                    body,
                    Sent::Prompts(list),
                    after,
                );
            }
        }
    }

    /// `ma`, then in Advanced Mode `dm`.
    fn onboarding_save_channels(&mut self, after: After, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding() else {
            return;
        };
        let guild = state.guild;
        if !state.channels_changed() {
            if state.advanced {
                self.onboarding_save_prompts(true, after, cx);
            } else {
                self.onboarding_run_after(after, None, cx);
            }
            return;
        }
        let exists = self.onboarding_channel_known(guild);
        let channels: Vec<String> = state
            .channels
            .iter()
            .filter(|id| exists(id))
            .cloned()
            .collect();
        let mut prompts = None;
        if state.advanced {
            let role = self.onboarding_role_known(guild);
            let known = self.stores.guild(guild);
            let has_emoji = |id: &str| {
                known.is_some_and(|known| {
                    known.emojis.iter().any(|emoji| emoji.id.to_string() == id)
                })
            };
            match save_list(
                &Known {
                    role: &role,
                    channel: &exists,
                },
                &state.saved.prompts,
                &state.prompts,
                true,
                &has_emoji,
            ) {
                SaveList::Send(list) => prompts = Some(list),
                SaveList::Invalid(errors) | SaveList::TooMany(errors) => {
                    if let Some(state) = self.onboarding_mut() {
                        state.errors = errors;
                    }
                    return;
                }
                SaveList::Nothing => {}
            }
        }
        let mut body = json!({ "default_channel_ids": channels });
        if let Some(list) = prompts.as_ref() {
            body["prompts"] = json!(list.iter().map(Prompt::to_wire).collect::<Vec<_>>());
        }
        self.onboarding_send(
            format!("/guilds/{guild}/onboarding"),
            reqwest::Method::PUT,
            body,
            Sent::Channels { channels, prompts },
            after,
        );
    }

    /// `W5` with `G`.
    fn onboarding_save_guide(&mut self, after: After, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding() else {
            return;
        };
        if !state.home_changed() {
            self.onboarding_run_after(after, None, cx);
            return;
        }
        let guild = state.guild;
        let viewable = self.onboarding_viewable(guild);
        if state.home_saved.enabled && !state.home.is_valid(&viewable) {
            self.onboarding_alert(GUIDE_SAVE_FAILED, GUIDE_INVALID.to_owned());
            return;
        }
        let home = state.home.clone();
        self.onboarding_put_guide(guild, home, after);
    }

    fn onboarding_put_guide(&mut self, guild: Id, home: Home, after: After) {
        let exists = self.onboarding_channel_known(guild);
        let body = home.to_wire(guild, &exists);
        self.onboarding_send(
            format!("/guilds/{guild}/new-member-welcome"),
            reqwest::Method::PUT,
            body,
            Sent::Guide(home),
            after,
        );
    }

    /// `UP`.
    fn onboarding_set_guide_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding() else {
            return;
        };
        let guild = state.guild;
        let viewable = self.onboarding_viewable(guild);
        if enabled && !state.home_saved.is_valid(&viewable) {
            self.onboarding_alert(GUIDE_SAVE_FAILED, GUIDE_INVALID.to_owned());
            return;
        }
        let mut home = state.home_saved.clone();
        home.enabled = enabled;
        if let Some(state) = self.onboarding_mut() {
            state.home.enabled = enabled;
            state.home_saved.enabled = enabled;
        }
        self.onboarding_put_guide(guild, home, After::Nothing);
        cx.notify();
    }

    fn onboarding_send_task_icon(&mut self, guild: Id, channel: &str, body: Value) {
        self.onboarding_send(
            format!("/guilds/{guild}/new-member-actions/{channel}"),
            reqwest::Method::PATCH,
            body,
            Sent::TaskIcon(channel.to_owned()),
            After::Nothing,
        );
    }

    /// `s` in `gZ`: `gr`, the switch or the big blue button.
    fn onboarding_toggle(&mut self, with_guide: bool, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding_mut() else {
            return;
        };
        let guild = state.guild;
        let enable = !state.saved.enabled;
        // `GUILD_ONBOARDING_PROMPTS_LOCAL_UPDATE`.
        state.saved.enabled = enable;
        let after = if enable && with_guide {
            After::EnableGuide
        } else {
            After::Nothing
        };
        self.onboarding_send(
            format!("/guilds/{guild}/onboarding"),
            reqwest::Method::PUT,
            json!({ "enabled": enable }),
            Sent::Enabled(enable),
            after,
        );
        cx.notify();
    }

    /// `CP`.
    fn onboarding_set_mode(&mut self, advanced: bool, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding() else {
            return;
        };
        let guild = state.guild;
        // `m7`'s effect depends on the mode: its cleanup saves what was checked when the mode changes.
        let flush = state.step == Step::Channels
            && !ever_enabled(self.stores.guild(guild))
            && state.channels_changed();
        if flush {
            self.onboarding_save_channels(After::Nothing, cx);
        }
        let mode = u64::from(advanced);
        self.onboarding_send(
            format!("/guilds/{guild}/onboarding"),
            reqwest::Method::PUT,
            json!({ "mode": mode }),
            Sent::Mode(mode),
            After::Nothing,
        );
        cx.notify();
    }

    /// `g9`'s first effect: Questions opens on one question, unless the server is in Advanced Mode.
    fn onboarding_seed_starter(&mut self) {
        let Some(state) = self.onboarding_mut() else {
            return;
        };
        if state.step == Step::Questions && state.loaded && !state.seeded_starter {
            state.seeded_starter = true;
            if !state.advanced && state.prompts.is_empty() {
                state.sequence += 1;
                state
                    .prompts
                    .push(Prompt::starter(local_id(state.sequence)));
            }
        }
    }

    /// The fields follow the questions (`is.k`'s `value`).
    fn onboarding_sync_fields(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding() else {
            return;
        };
        let prompts: Vec<(String, String)> = state
            .prompts
            .iter()
            .map(|prompt| (prompt.id.clone(), prompt.title.clone()))
            .collect();
        let missing: Vec<(String, String)> = prompts
            .iter()
            .filter(|(id, _)| !state.fields.contains_key(id))
            .cloned()
            .collect();
        let seen: Vec<String> = prompts
            .iter()
            .filter(|(_, title)| !title.is_empty())
            .map(|(id, _)| id.clone())
            .collect();
        let mut made = Vec::new();
        for (id, title) in missing {
            let prompt = id.clone();
            let field = self.onboarding_field(
                "Enter a question...",
                &title,
                100,
                false,
                move |this, text, cx| this.onboarding_title_edited(&prompt, text, cx),
                cx,
            );
            made.push((id, field));
        }
        let Some(state) = self.onboarding_mut() else {
            return;
        };
        state.fields.extend(made);
        // `m$`: the examples go for good once the question has words.
        state.examples_shown.extend(seen);
        state
            .fields
            .retain(|id, _| prompts.iter().any(|(known, _)| known == id));
        for (id, title) in &prompts {
            if let Some(field) = state.fields.get(id)
                && field.editor.read(cx).text() != title
            {
                field.editor.update(cx, |editor, _| editor.set_text(title));
            }
        }
    }

    /// One input (`is.k`, or the text area `x.f`): reports each edit, clips to `maxLength`.
    fn onboarding_field(
        &mut self,
        placeholder: &str,
        text: &str,
        limit: usize,
        multiline: bool,
        edited: impl Fn(&mut Self, String, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Field {
        let placeholder = placeholder.to_owned();
        let style = if multiline {
            field_style(16., 21.)
        } else {
            field_style(16., 20.)
        };
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder(placeholder);
            editor.set_text(text);
            editor.set_reports_all_edits(true);
            editor.set_enter_inserts_newline(multiline);
            editor
        });
        let events = cx.subscribe(&editor, move |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {
                    let mut text = editor.read(cx).text().to_owned();
                    if let Some(short) = clipped(&text, limit) {
                        editor.update(cx, |editor, _| editor.set_text(&short));
                        text = short;
                    }
                    edited(this, text, cx);
                }
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                }
                EditorEvent::Cancel => {
                    if !this.close_onboarding_overlay() {
                        this.onboarding_leave(cx);
                        this.server_settings = None;
                    }
                }
            }
            cx.notify();
        });
        Field {
            editor,
            _events: events,
        }
    }

    /// `N` with validation.
    fn onboarding_edit_prompts(
        &mut self,
        change: impl FnOnce(&mut Vec<Prompt>),
        validate: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.onboarding() else {
            return;
        };
        let guild = state.guild;
        let role = self.onboarding_role_known(guild);
        let channel = self.onboarding_channel_known(guild);
        if let Some(state) = self.onboarding_mut() {
            change(&mut state.prompts);
            if validate {
                let known = Known {
                    role: &role,
                    channel: &channel,
                };
                state.errors = state
                    .prompts
                    .iter()
                    .map(|prompt| prompt_error(&known, &state.prompts, prompt))
                    .collect();
            }
        }
        self.onboarding_sync_fields(cx);
        self.onboarding_request_pictures();
        cx.notify();
    }

    /// `NF`.
    fn onboarding_edit_prompt(
        &mut self,
        id: &str,
        change: impl FnOnce(&mut Prompt),
        cx: &mut Context<Self>,
    ) {
        self.onboarding_edit_prompts(
            |prompts| {
                if let Some(prompt) = prompts.iter_mut().find(|prompt| prompt.id == id) {
                    change(prompt);
                }
            },
            true,
            cx,
        );
    }

    fn onboarding_title_edited(&mut self, id: &str, text: String, cx: &mut Context<Self>) {
        if self
            .onboarding()
            .and_then(|state| state.prompts.iter().find(|prompt| prompt.id == id))
            .is_some_and(|prompt| prompt.title != text)
        {
            self.onboarding_edit_prompt(id, |prompt| prompt.title = text, cx);
        }
    }

    /// Unmounting a step: `m7`, `g9` and `gT`'s cleanups. Only the Server Guide's runs once Onboarding was ever on.
    pub(super) fn onboarding_leave(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding() else {
            return;
        };
        let guild = state.guild;
        let step = state.step;
        let close_save = !ever_enabled(self.stores.guild(guild));
        match step {
            Step::Channels if close_save => self.onboarding_save_channels(After::Nothing, cx),
            Step::Questions if close_save => self.onboarding_save_prompts(true, After::Nothing, cx),
            Step::Home => self.onboarding_save_guide(After::Nothing, cx),
            _ => {}
        }
    }

    /// `e_`, with the unmount saves.
    fn onboarding_go(&mut self, step: Step, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.onboarding_leave(cx);
        self.onboarding_enter(step, window, cx);
    }

    fn onboarding_enter(
        &mut self,
        step: Step,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.onboarding_mut() {
            state.step = step;
            state.dropdown = None;
            state.seeded_starter = false;
        }
        self.onboarding_seed_starter();
        self.onboarding_sync_fields(cx);
        // `mQ`'s layout effect focuses the first question.
        if step == Step::Questions
            && let Some(window) = window
            && let Some(handle) = self.onboarding().and_then(|state| {
                state
                    .prompts
                    .iter()
                    .find(|prompt| prompt.in_onboarding)
                    .and_then(|prompt| state.fields.get(&prompt.id))
                    .map(|field| field.editor.read(cx).focus_handle(cx))
            })
        {
            window.focus(&handle, cx);
        }
        cx.notify();
    }

    /// `f` in `hh`, then `_k` or `kP`.
    fn onboarding_step_button(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding_mut() else {
            return;
        };
        let step = state.step;
        let target = if back {
            previous_step(step)
        } else {
            next_step(step)
        };
        let Some(target) = target else {
            return;
        };
        if !back && let Some(name) = state.upsell_name_for(step) {
            state.upsells_dismissed.insert(name);
        }
        let changed = state.step_changed();
        if !changed {
            self.onboarding_enter(target, Some(window), cx);
            return;
        }
        match step {
            Step::Channels => self.onboarding_save_channels(After::Go(target), cx),
            Step::Questions => self.onboarding_save_prompts(true, After::Go(target), cx),
            Step::Home => self.onboarding_save_guide(After::Go(target), cx),
            _ => self.onboarding_enter(target, Some(window), cx),
        }
        cx.notify();
    }

    /// The save bar's Save (`m8`, `g7`, `hc`).
    pub(super) fn save_onboarding(&mut self, cx: &mut Context<Self>) {
        let Some(step) = self.onboarding().map(|state| state.step) else {
            return;
        };
        match step {
            Step::Channels => self.onboarding_save_channels(After::Nothing, cx),
            Step::Questions => self.onboarding_save_prompts(false, After::Nothing, cx),
            _ => self.onboarding_save_guide(After::Nothing, cx),
        }
        cx.notify();
    }

    /// The save bar's Reset: `ms`, `P3`, `sk`.
    pub(super) fn reset_onboarding(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.onboarding_mut() {
            state.channels = state.saved.default_channels.iter().cloned().collect();
            state.prompts = state.saved.prompts.clone();
            state.errors.clear();
            state.home = state.home_saved.clone();
            state.pending_icons.clear();
        }
        self.onboarding_sync_fields(cx);
        cx.notify();
    }

    /// `uw.showNotice` after Onboarding was ever on.
    pub(super) fn onboarding_dirty(&self) -> bool {
        let Some(state) = self.onboarding() else {
            return false;
        };
        ever_enabled(self.stores.guild(state.guild))
            && (state.step_changed()
                || (matches!(state.step, Step::Landing | Step::Review) && state.home_changed()))
    }

    /// Escape: a dropdown, then a confirmation, then a dialog.
    pub(super) fn close_onboarding_overlay(&mut self) -> bool {
        let Some(state) = self.onboarding_mut() else {
            return false;
        };
        if state.dropdown.take().is_some() {
            return true;
        }
        if state.confirm.take().is_some() {
            return true;
        }
        state.dialog.take().is_some()
    }

    /// Root render: which of the page's fields has focus.
    pub(super) fn onboarding_note_focus(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding() else {
            return;
        };
        let mut fields: Vec<Entity<Editor>> = state
            .fields
            .values()
            .map(|field| field.editor.clone())
            .collect();
        match state.dialog.as_ref() {
            Some(Dialog::Answer(dialog)) => {
                fields.push(dialog.title.editor.clone());
                fields.push(dialog.description.editor.clone());
                fields.push(dialog.channel_query.editor.clone());
                fields.push(dialog.role_query.editor.clone());
            }
            Some(Dialog::Task(dialog)) => fields.push(dialog.title.editor.clone()),
            Some(Dialog::Resource(dialog)) => {
                fields.push(dialog.title.editor.clone());
                fields.push(dialog.description.editor.clone());
            }
            Some(Dialog::Welcome(dialog)) => fields.push(dialog.message.editor.clone()),
            _ => {}
        }
        let focused = fields
            .iter()
            .find(|editor| editor.read(cx).focus_handle(cx).is_focused(window))
            .map(Entity::entity_id);
        if let Some(state) = self.onboarding_mut() {
            state.focused = focused;
        }
    }

    /// The channel row's click: toggle, or offer to open it to @everyone (`mS`, `mT`, `m4`).
    fn onboarding_row_clicked(&mut self, guild: Id, channel: Id, cx: &mut Context<Self>) {
        let Some(known) = self.stores.guild(guild) else {
            return;
        };
        let Some(found) = known.channels.iter().find(|c| c.id == channel) else {
            return;
        };
        if can_default(known, found) || found.kind == ChannelKind::Category {
            let id = channel.to_string();
            if let Some(state) = self.onboarding_mut()
                && !state.channels.remove(&id)
            {
                state.channels.insert(id);
            }
        } else {
            self.onboarding_open_unlock(guild, channel);
        }
        cx.notify();
    }

    fn onboarding_open_unlock(&mut self, guild: Id, start: Id) {
        let Some(known) = self.stores.guild(guild) else {
            return;
        };
        let mut channels: Vec<&Channel> = known
            .channels
            .iter()
            .filter(|channel| {
                (is_selectable(channel) || is_vocal(channel)) && is_private(known, channel)
            })
            .collect();
        channels.sort_by_key(|channel| (is_vocal(channel), channel.position, channel.id));
        let channels: Vec<Id> = channels.iter().map(|channel| channel.id).collect();
        let selected = channels.iter().copied().filter(|id| *id == start).collect();
        if let Some(state) = self.onboarding_mut() {
            state.dialog = Some(Dialog::Unlock(UnlockDialog {
                reviewing: false,
                channels,
                selected,
            }));
        }
    }

    /// `K` in module 147696.
    fn onboarding_unlock_selected(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding() else {
            return;
        };
        let guild = state.guild;
        let Some(Dialog::Unlock(dialog)) = state.dialog.as_ref() else {
            return;
        };
        let selected = dialog.selected.clone();
        for id in selected {
            let Some(body) = self
                .stores
                .guild(guild)
                .and_then(|known| known.channels.iter().find(|channel| channel.id == id))
                .map(|channel| unlocked_overwrite(guild, channel))
            else {
                continue;
            };
            self.onboarding_send(
                format!("/channels/{id}/permissions/{guild}"),
                reqwest::Method::PUT,
                body,
                Sent::Unlock(id),
                After::Nothing,
            );
        }
        if let Some(state) = self.onboarding_mut() {
            state.dialog = None;
        }
        cx.notify();
    }

    /// `mq`/`mX`'s dialog.
    fn onboarding_open_answer(
        &mut self,
        prompt: &str,
        answer: Option<&str>,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(found) = self
            .onboarding()
            .and_then(|state| state.prompts.iter().find(|known| known.id == prompt))
            .cloned()
        else {
            return;
        };
        let existing = answer.and_then(|id| found.options.iter().find(|option| option.id == id));
        let title = self.onboarding_field(
            "Enter an answer...",
            existing.map_or("", |option| option.title.as_str()),
            50,
            false,
            |this, _, cx| {
                if let Some(Dialog::Answer(dialog)) = this
                    .onboarding_mut()
                    .and_then(|state| state.dialog.as_mut())
                {
                    dialog.title_error = None;
                }
                cx.notify();
            },
            cx,
        );
        let description = self.onboarding_field(
            "Enter a description... (optional)",
            existing.map_or("", |option| option.description.as_str()),
            100,
            false,
            |_, _, _| {},
            cx,
        );
        let channel_query = self.onboarding_field(
            "Enter channels...",
            "",
            100,
            false,
            |this, _, cx| {
                if let Some(state) = this.onboarding_mut() {
                    state.dropdown = Some(Dropdown::Channels);
                }
                cx.notify();
            },
            cx,
        );
        let role_query = self.onboarding_field(
            "Enter roles...",
            "",
            100,
            false,
            |this, _, cx| {
                if let Some(state) = this.onboarding_mut() {
                    state.dropdown = Some(Dropdown::Roles);
                }
                cx.notify();
            },
            cx,
        );
        let handle = title.editor.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        let dialog = AnswerDialog {
            prompt: prompt.to_owned(),
            answer: answer.map(str::to_owned),
            index,
            prompt_title: if found.title.is_empty() {
                DEFAULT_PROMPT_TITLE.to_owned()
            } else {
                found.title.clone()
            },
            title,
            description,
            channels: existing.map_or(Vec::new(), |option| {
                let mut list = option.channel_ids.clone();
                list.sort();
                list
            }),
            roles: existing.map_or(Vec::new(), |option| {
                let mut list = option.role_ids.clone();
                list.sort();
                list
            }),
            channel_query,
            role_query,
            emoji: existing.and_then(|option| option.emoji.clone()),
            title_error: None,
            roles_error: None,
        };
        if let Some(state) = self.onboarding_mut() {
            state.dialog = Some(Dialog::Answer(Box::new(dialog)));
            state.dropdown = None;
        }
        cx.notify();
    }

    /// `Y` in module 165394.
    fn onboarding_save_answer(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding() else {
            return;
        };
        let guild = state.guild;
        let Some(Dialog::Answer(dialog)) = state.dialog.as_ref() else {
            return;
        };
        let mut channels = dialog.channels.clone();
        channels.sort();
        let mut roles = dialog.roles.clone();
        roles.sort();
        let answer = Answer {
            id: dialog
                .answer
                .clone()
                .unwrap_or_else(|| local_id(state.sequence + 1)),
            title: dialog.title.text(cx),
            description: dialog.description.text(cx),
            channel_ids: channels,
            role_ids: roles,
            emoji: dialog.emoji.clone(),
        };
        let prompt_id = dialog.prompt.clone();
        let is_new = dialog.answer.is_none();
        let Some(prompt) = state.prompts.iter().find(|known| known.id == prompt_id) else {
            return;
        };
        let role = self.onboarding_role_known(guild);
        let channel = self.onboarding_channel_known(guild);
        let known = Known {
            role: &role,
            channel: &channel,
        };
        let title_error = answer.title.is_empty().then_some("Title is required");
        let roles_error = answer_error(&known, &state.prompts, prompt, &answer);
        if title_error.is_some() || roles_error.is_some() {
            if let Some(Dialog::Answer(dialog)) = self
                .onboarding_mut()
                .and_then(|state| state.dialog.as_mut())
            {
                dialog.title_error = title_error;
                dialog.roles_error = roles_error;
            }
            cx.notify();
            return;
        }
        if let Some(state) = self.onboarding_mut() {
            state.sequence += 1;
            state.dialog = None;
            state.dropdown = None;
        }
        self.onboarding_edit_prompt(
            &prompt_id,
            |prompt| {
                if is_new {
                    prompt.options.push(answer);
                } else if let Some(found) = prompt
                    .options
                    .iter_mut()
                    .find(|known| known.id == answer.id)
                {
                    *found = answer;
                }
            },
            cx,
        );
    }

    fn onboarding_remove_answer(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Answer(dialog)) =
            self.onboarding().and_then(|state| state.dialog.as_ref())
        else {
            return;
        };
        let prompt = dialog.prompt.clone();
        let answer = dialog.answer.clone();
        if let Some(state) = self.onboarding_mut() {
            state.dialog = None;
            state.dropdown = None;
        }
        // `onDelete` is a no-op for a new answer: `() => {}` in `mq`.
        if let Some(answer) = answer {
            self.onboarding_edit_prompt(
                &prompt,
                |prompt| prompt.options.retain(|option| option.id != answer),
                cx,
            );
        }
        cx.notify();
    }

    /// `ga`/`gl`'s dialog.
    fn onboarding_open_task(
        &mut self,
        editing: Option<&Task>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = self.onboarding_field(
            "Ex: Post a photo of your pet",
            editing.map_or("", |task| task.title.as_str()),
            60,
            false,
            |this, _, cx| {
                this.onboarding_task_recheck();
                cx.notify();
            },
            cx,
        );
        let _ = window;
        let pending = editing.and_then(|task| {
            self.onboarding()
                .and_then(|state| state.pending_icons.get(&task.channel_id).cloned())
        });
        let icon = pending.as_ref().and_then(|(icon, _)| icon.clone());
        let emoji = editing.and_then(|task| task.emoji.clone());
        let dialog = TaskDialog {
            editing: editing.map(|task| task.channel_id.clone()),
            title,
            channel: editing.map(|task| task.channel_id.clone()),
            action: editing.map(|task| task.action_type),
            first_emoji: emoji.clone(),
            emoji,
            first_icon: icon.clone(),
            icon,
            icon_key: editing.and_then(Self::task_icon_url),
            action_error: None,
        };
        if let Some(state) = self.onboarding_mut() {
            state.dialog = Some(Dialog::Task(Box::new(dialog)));
            state.dropdown = None;
        }
        cx.notify();
    }

    /// `z`'s second half: re-read when the channel or the choice changes.
    fn onboarding_task_recheck(&mut self) {
        let Some(state) = self.onboarding() else {
            return;
        };
        let guild = state.guild;
        let Some(Dialog::Task(dialog)) = state.dialog.as_ref() else {
            return;
        };
        let error = match (dialog.action, dialog.channel.as_ref()) {
            (Some(action), Some(channel)) => self.stores.guild(guild).and_then(|known| {
                let found = known
                    .channels
                    .iter()
                    .find(|c| c.id.to_string() == *channel)?;
                (!can_complete(known, found, action)).then_some(if action == 0 {
                    "The selected channel does not have view channel permissions for @everyone."
                } else {
                    "The selected channel does not have send message permissions for @everyone."
                })
            }),
            _ => None,
        };
        if let Some(Dialog::Task(dialog)) = self
            .onboarding_mut()
            .and_then(|state| state.dialog.as_mut())
        {
            dialog.action_error = error;
        }
    }

    /// `eI` in module 251632, then `l` or `i` in `go`.
    fn onboarding_save_task(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding() else {
            return;
        };
        let guild = state.guild;
        let Some(Dialog::Task(dialog)) = state.dialog.as_ref() else {
            return;
        };
        let (Some(channel), Some(action)) = (dialog.channel.clone(), dialog.action) else {
            return;
        };
        let title = dialog.title.text(cx);
        let moved = dialog.editing.as_ref().is_some_and(|old| *old != channel);
        let changed =
            (dialog.emoji != dialog.first_emoji || dialog.icon != dialog.first_icon) && !moved;
        let editing = dialog.editing.clone();
        let emoji = dialog.emoji.clone();
        let icon = dialog.icon.clone();
        let current_icon = editing.as_ref().and_then(|old| {
            state
                .home
                .tasks
                .iter()
                .flatten()
                .find(|task| task.channel_id == *old)
                .and_then(|task| task.icon.clone())
        });
        let had_pending = editing
            .as_ref()
            .is_some_and(|old| state.pending_icons.contains_key(old));
        let task = Task {
            channel_id: channel.clone(),
            action_type: action,
            title,
            description: String::new(),
            emoji: emoji.clone(),
            icon: current_icon,
        };
        let mut send_now = None;
        if let Some(state) = self.onboarding_mut() {
            state.dialog = None;
            state.dropdown = None;
            match editing {
                None => {
                    state.pending_icons.insert(channel.clone(), (icon, emoji));
                    state.home.tasks.get_or_insert_default().push(task);
                }
                Some(old) => {
                    if moved || had_pending {
                        state.pending_icons.remove(&old);
                        state
                            .pending_icons
                            .insert(channel.clone(), (icon.clone(), emoji.clone()));
                    }
                    if !state.pending_icons.contains_key(&channel) && changed {
                        send_now = Some((
                            old.clone(),
                            if emoji.is_some() {
                                json!({ "emoji": emoji.as_ref().map(|emoji| json!({
                                    "id": emoji.id, "name": emoji.name, "animated": emoji.animated,
                                })) })
                            } else {
                                json!({ "icon": icon })
                            },
                        ));
                    }
                    if let Some(found) = state
                        .home
                        .tasks
                        .iter_mut()
                        .flatten()
                        .find(|task| task.channel_id == old)
                    {
                        *found = task;
                    }
                }
            }
        }
        if let Some((old, body)) = send_now {
            self.onboarding_send_task_icon(guild, &old, body);
        }
        self.onboarding_request_pictures();
        cx.notify();
    }

    fn onboarding_remove_task(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.onboarding_mut() {
            if let Some(Dialog::Task(dialog)) = state.dialog.as_ref()
                && let Some(old) = dialog.editing.clone()
            {
                if let Some(tasks) = state.home.tasks.as_mut() {
                    tasks.retain(|task| task.channel_id != old);
                }
                state.pending_icons.remove(&old);
            }
            state.dialog = None;
            state.dropdown = None;
        }
        cx.notify();
    }

    /// `gf`/`gp`'s dialog.
    fn onboarding_open_resource(&mut self, editing: Option<&Resource>, cx: &mut Context<Self>) {
        let title = self.onboarding_field(
            "#rules might be Rules",
            editing.map_or("", |resource| resource.title.as_str()),
            30,
            false,
            |_, _, cx| cx.notify(),
            cx,
        );
        let description = self.onboarding_field(
            "",
            editing.map_or("", |resource| resource.description.as_str()),
            200,
            true,
            |_, _, _| {},
            cx,
        );
        let dialog = ResourceDialog {
            editing: editing.map(|resource| resource.channel_id.clone()),
            channel: editing.map(|resource| resource.channel_id.clone()),
            title,
            description,
            icon: editing.and_then(|resource| resource.icon.clone()),
        };
        if let Some(state) = self.onboarding_mut() {
            state.dialog = Some(Dialog::Resource(Box::new(dialog)));
            state.dropdown = None;
        }
        cx.notify();
    }

    /// `Y` in module 983161, with `px`/`z4`.
    fn onboarding_save_resource(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding_mut() else {
            return;
        };
        let Some(Dialog::Resource(dialog)) = state.dialog.as_ref() else {
            return;
        };
        let Some(channel) = dialog.channel.clone() else {
            return;
        };
        let title = dialog.title.text(cx);
        if title.is_empty() {
            return;
        }
        // `G`: the icon the store has now, which an upload from this dialog may have changed.
        let icon = state
            .home
            .resources
            .iter()
            .flatten()
            .find(|known| known.channel_id == channel)
            .and_then(|known| known.icon.clone())
            .or_else(|| dialog.icon.clone());
        let resource = Resource {
            channel_id: channel.clone(),
            title,
            description: dialog.description.text(cx),
            emoji: None,
            icon,
        };
        let editing = dialog.editing.clone();
        state.dialog = None;
        state.dropdown = None;
        let resources = state.home.resources.get_or_insert_default();
        match editing {
            Some(old) => {
                if let Some(found) = resources.iter_mut().find(|known| known.channel_id == old) {
                    *found = resource;
                }
            }
            None => {
                resources.retain(|known| known.channel_id != channel);
                resources.push(resource);
            }
        }
        cx.notify();
    }

    fn onboarding_remove_resource(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.onboarding_mut() {
            if let Some(Dialog::Resource(dialog)) = state.dialog.as_ref()
                && let Some(old) = dialog.editing.clone()
                && let Some(resources) = state.home.resources.as_mut()
            {
                resources.retain(|resource| resource.channel_id != old);
            }
            state.dialog = None;
            state.dropdown = None;
        }
        cx.notify();
    }

    /// `Q` and `o`/`A`: an upload saves the guide, then puts the picture on the channel.
    fn onboarding_resource_upload(&mut self, uri: String, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding_mut() else {
            return;
        };
        let guild = state.guild;
        let Some(Dialog::Resource(dialog)) = state.dialog.as_ref() else {
            return;
        };
        let Some(channel) = dialog.channel.clone() else {
            return;
        };
        let resource = Resource {
            channel_id: channel.clone(),
            title: dialog.title.text(cx),
            description: dialog.description.text(cx),
            emoji: None,
            icon: None,
        };
        let resources = state.home.resources.get_or_insert_default();
        resources.retain(|known| known.channel_id != channel);
        resources.push(resource);
        let home = state.home.clone();
        self.onboarding_put_guide(guild, home, After::ResourceIcon { channel, uri });
        cx.notify();
    }

    /// `gv`'s dialog.
    fn onboarding_open_welcome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding() else {
            return;
        };
        let welcome = state.home.welcome.clone().unwrap_or_default();
        let message =
            self.onboarding_field("", &welcome.message, 300, true, |_, _, cx| cx.notify(), cx);
        let _ = window;
        if let Some(state) = self.onboarding_mut() {
            state.dialog = Some(Dialog::Welcome(Box::new(WelcomeDialog {
                author: welcome.author_ids.first().cloned(),
                message,
            })));
            state.dropdown = None;
        }
        cx.notify();
    }

    /// `N` and `P` in module 930345, with `hL`.
    fn onboarding_save_welcome(&mut self, remove: bool, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding_mut() else {
            return;
        };
        let Some(Dialog::Welcome(dialog)) = state.dialog.as_ref() else {
            return;
        };
        let welcome = (!remove).then(|| Welcome {
            author_ids: dialog.author.iter().cloned().collect(),
            message: dialog.message.text(cx),
        });
        state.home.welcome = welcome;
        state.dialog = None;
        state.dropdown = None;
        self.onboarding_request_pictures();
        cx.notify();
    }

    /// Picked in the emoji picker for an answer or a task.
    pub(super) fn set_onboarding_emoji(
        &mut self,
        id: Option<String>,
        name: Option<String>,
        animated: bool,
        cx: &mut Context<Self>,
    ) {
        let emoji = Emoji {
            id,
            name,
            animated: Some(animated),
        };
        self.onboarding_request_emoji(&emoji);
        if let Some(state) = self.onboarding_mut() {
            match state.dialog.as_mut() {
                Some(Dialog::Answer(dialog)) => dialog.emoji = Some(emoji),
                Some(Dialog::Task(dialog)) => {
                    dialog.emoji = Some(emoji);
                    dialog.icon = None;
                    dialog.icon_key = None;
                }
                _ => {}
            }
        }
        cx.notify();
    }

    fn onboarding_pick_picture(
        &mut self,
        done: impl Fn(&mut Self, String, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(path) = paths.first()
                && let Some(uri) = super::server_settings::data_uri(path)
            {
                let _ = this.update(cx, |this, cx| done(this, uri, cx));
            }
        })
        .detach();
    }

    /// A picked file is shown before it is sent, so its bytes join the image store under the data URI.
    pub(super) fn onboarding_keep_picture(&mut self, uri: &str) {
        use base64::Engine;
        let Some((head, data)) = uri.split_once(";base64,") else {
            return;
        };
        let format = match head {
            "data:image/png" => gpui::ImageFormat::Png,
            "data:image/jpeg" => gpui::ImageFormat::Jpeg,
            "data:image/gif" => gpui::ImageFormat::Gif,
            "data:image/webp" => gpui::ImageFormat::Webp,
            _ => return,
        };
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
            self.images.insert(
                uri.to_owned(),
                std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)),
            );
        }
    }

    fn onboarding_task_picture(&mut self, uri: String, cx: &mut Context<Self>) {
        self.onboarding_keep_picture(&uri);
        if let Some(Dialog::Task(dialog)) = self
            .onboarding_mut()
            .and_then(|state| state.dialog.as_mut())
        {
            dialog.icon = Some(uri);
            dialog.emoji = None;
        }
        cx.notify();
    }

    /// `ex.A.saveGuild(id, { homeHeader })`.
    fn onboarding_home_header(&mut self, uri: Option<String>, cx: &mut Context<Self>) {
        let Some(guild) = self.onboarding().map(|state| state.guild) else {
            return;
        };
        self.onboarding_send(
            format!("/guilds/{guild}"),
            reqwest::Method::PATCH,
            json!({ "home_header": uri }),
            Sent::HomeHeader,
            After::Nothing,
        );
        cx.notify();
    }
}

impl OnboardingState {
    fn upsell_name_for(&self, step: Step) -> Option<&'static str> {
        match step {
            Step::Safety => Some("Safety Check"),
            Step::Channels => Some("Default Channels"),
            Step::Questions => Some("Customization Questions"),
            Step::Home => Some("Home Settings"),
            _ => None,
        }
    }
}

/// Text in one of the design system's variants.
fn text(size: f32, line: f32, weight: f32, tint: &str) -> gpui::Div {
    div()
        .text_size(px(size))
        .line_height(px(line))
        .font_weight(FontWeight(weight))
        .text_color(color(tint))
}

fn heading_lg(title: impl Into<SharedString>) -> gpui::Div {
    text(20., 24., 600., "--text-strong").child(title.into())
}

fn heading_sm(title: impl Into<SharedString>) -> gpui::Div {
    text(16., 20., 600., "--text-strong").child(title.into())
}

fn body_sm(content: impl Into<SharedString>) -> gpui::Div {
    text(14., 18., 400., "--text-default").child(content.into())
}

fn body_sm_medium(content: impl Into<SharedString>) -> gpui::Div {
    text(14., 18., 500., "--text-default").child(content.into())
}

fn link(content: impl Into<SharedString>) -> gpui::Div {
    text(14., 18., 500., "--text-link")
        .cursor_pointer()
        .child(content.into())
}

/// `dot_2nqf8W`.
fn link_dot() -> gpui::Div {
    div()
        .size(px(4.))
        .flex_none()
        .rounded(px(8.))
        .bg(color("--background-mod-subtle"))
}

/// `divider_ZaYWLb` and friends: `--border-subtle`, one pixel.
fn rule() -> gpui::Div {
    div()
        .h(px(1.))
        .flex_none()
        .w_full()
        .bg(color("--border-subtle"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Primary,
    Secondary,
}

/// `D.$` in sizes sm and md (`button_F7qvhY`).
fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    look: Look,
    medium: bool,
    disabled: bool,
) -> Stateful<gpui::Div> {
    let (background, border, tint) = match look {
        Look::Primary => (
            "--control-primary-background-default",
            "--control-primary-border-default",
            "--white",
        ),
        Look::Secondary => (
            "--control-secondary-background-default",
            "--control-secondary-border-default",
            "--control-secondary-text-default",
        ),
    };
    div()
        .id(id.into())
        .flex_none()
        .h(px(if medium { 40. } else { 32. }))
        .px(px(if medium { 16. } else { 12. }))
        .rounded(px(8.))
        .border_1()
        .border_color(color(border))
        .bg(color(background))
        .when(disabled, |this| this.opacity(0.5))
        .when(!disabled, |this| this.cursor_pointer())
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .text_size(px(if medium { 16. } else { 14. }))
        .line_height(px(if medium { 20. } else { 18. }))
        .font_weight(FontWeight(500.))
        .text_color(color(tint))
        .child(label.into())
}

/// `D.$` with an icon only.
fn icon_button(
    id: impl Into<SharedString>,
    glyph: &str,
    look: Look,
    size: f32,
) -> Stateful<gpui::Div> {
    let (background, border, tint) = match look {
        Look::Primary => (
            "--control-primary-background-default",
            "--control-primary-border-default",
            "--white",
        ),
        Look::Secondary => (
            "--control-secondary-background-default",
            "--control-secondary-border-default",
            "--control-secondary-text-default",
        ),
    };
    div()
        .id(id.into())
        .flex_none()
        .size(px(size))
        .rounded(px(8.))
        .border_1()
        .border_color(color(border))
        .bg(color(background))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .child(icon(glyph, if size > 32. { 20. } else { 16. }, tint))
}

/// `ni.P`, the indicator 20 square with its 18 check.
pub(super) fn checkbox(checked: bool, disabled: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id("checkbox")
        .relative()
        .flex_none()
        .size(px(20.))
        .rounded(px(4.))
        .border_1()
        .border_color(if checked {
            color("--checkbox-border-selected-default")
        } else {
            color("--checkbox-border-default")
        })
        .bg(if checked {
            color("--checkbox-background-selected-default")
        } else {
            color("--checkbox-background-default")
        })
        .when(disabled, |this| this.opacity(0.5))
        .child(super::motion::controls::checkbox_marks(
            checked,
            icon("automod_check", 18., "--checkbox-icon-active"),
            color("--checkbox-icon-active").into(),
        ))
}

/// `mh.gm`.
pub(super) fn channel_glyph(guild: &Guild, channel: &Channel, rules: Option<Id>) -> &'static str {
    if Some(channel.id) == rules {
        return "ob_rules";
    }
    match channel.kind {
        ChannelKind::Voice | ChannelKind::Other(13) => "ob_voice",
        ChannelKind::Other(5) => "ob_announce",
        ChannelKind::Other(15 | 16) => "ob_forum",
        ChannelKind::Text if is_private(guild, channel) => "ob_text_limited",
        _ => "ob_text",
    }
}

fn shadow_high() -> Vec<gpui::BoxShadow> {
    vec![
        gpui::BoxShadow {
            color: gpui::hsla(0., 0., 1., 0.08),
            offset: gpui::point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(1.),
            inset: false,
        },
        gpui::BoxShadow {
            color: gpui::hsla(0., 0., 0., 0.24),
            offset: gpui::point(px(0.), px(12.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}

impl Root {
    /// `ht` and `hn`.
    pub(super) fn server_onboarding_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.onboarding() else {
            return div().into_any_element();
        };
        if state.guild != guild {
            return div().into_any_element();
        }
        let ever = ever_enabled(self.stores.guild(guild));
        let step = state.step;
        let body = match step {
            Step::Landing if ever => self.onboarding_review(guild, true, cx),
            Step::Landing => self.onboarding_landing(guild, cx),
            Step::Safety => self.community_safety_page(guild, cx),
            Step::Channels => self.onboarding_channels(guild, cx),
            Step::Questions => self.onboarding_questions(guild, cx),
            Step::Home => self.onboarding_guide(guild, cx),
            Step::Review => self.onboarding_review(guild, ever, cx),
        };
        let dim = !ever && state.upsell().is_some();
        div()
            .relative()
            .w_full()
            // The shared scroller ends 12px before this page's own padding does.
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(32.))
            .when(!ever && step != Step::Landing, |this| {
                this.child(self.onboarding_progress(state, cx))
            })
            .when(ever && step != Step::Landing, |this| {
                this.child(
                    div().child(
                        button("onboarding-back-top", "Back", Look::Secondary, false, false)
                            .w(px(73.9))
                            .child(icon(
                                "ob_arrow_left",
                                16.,
                                "--control-secondary-text-default",
                            ))
                            .flex_row_reverse()
                            .on_click(cx.listener(|this, _, window, cx| {
                                let blocked = this.onboarding().is_some_and(|state| {
                                    state.has_errors() || state.step_changed()
                                });
                                // `SHAKE_APP` and `EMPHASIZE_NOTICE`: nothing moves here (see the file's list).
                                if !blocked {
                                    this.reset_onboarding(cx);
                                    this.onboarding_enter(Step::Landing, Some(window), cx);
                                }
                            })),
                    ),
                )
            })
            .child(div().child(body))
            // `upsellOnTop_r_cyVx` is opacity .4 over one background, which this paints exactly, and no pointer events.
            .when(dim, |this| {
                let mut veil = color("--background-base-low");
                veil.a = 0.6;
                this.child(
                    div()
                        .id("onboarding-veil")
                        .occlude()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(veil),
                )
            })
            .into_any_element()
    }

    /// `hi`.
    fn onboarding_progress(&self, state: &OnboardingState, cx: &mut Context<Self>) -> AnyElement {
        let advanced = state.advanced;
        let items: [(Step, String); 5] = [
            (Step::Safety, "1. Safety Check".to_owned()),
            (
                Step::Channels,
                if advanced {
                    "2. Default Channels & Pre-join Questions".to_owned()
                } else {
                    "2. Default Channels".to_owned()
                },
            ),
            (
                Step::Questions,
                if advanced {
                    "3. Post-join Questions".to_owned()
                } else {
                    "3. Customization Questions".to_owned()
                },
            ),
            (Step::Home, "4. Server Guide".to_owned()),
            (Step::Review, "5. Review".to_owned()),
        ];
        let current = state.step;
        let mut row = div().flex().items_start().gap(px(8.));
        for (target, label) in items {
            let on = target == current;
            let clickable = target != Step::Review;
            row = row.child(
                div()
                    .id(SharedString::from(format!("onboarding-progress-{label}")))
                    .flex_1()
                    .min_w(px(0.))
                    .h(px(44.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .when(clickable, |this| this.cursor_pointer())
                    .child(div().h(px(4.)).w_full().rounded(px(8.)).bg(color(if on {
                        "--text-brand"
                    } else {
                        "--background-mod-muted"
                    })))
                    .child(
                        text(
                            12.,
                            16.,
                            500.,
                            if on { "--text-brand" } else { "--text-muted" },
                        )
                        .child(label),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let blocked = this.onboarding().is_some_and(OnboardingState::has_errors);
                        if clickable && !blocked {
                            this.onboarding_go(target, Some(window), cx);
                        }
                    })),
            );
        }
        row.into_any_element()
    }

    /// The bar `hh` draws over the page while the wizard runs, with `hr`, `ha`, `ho` or `hd` above it.
    pub(super) fn onboarding_notice(
        &self,
        guild: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if open.page != Page::Onboarding {
            return None;
        }
        let state = open.onboarding.as_ref()?;
        if state.guild != guild || ever_enabled(self.stores.guild(guild)) {
            return None;
        }
        // `showNotice`: the Safety step's sub-pages have none.
        if state.step == Step::Landing
            || (state.step == Step::Safety
                && open.safety.step != super::server_safety_community::SafetyStep::Overview)
        {
            return None;
        }
        let upsell = state.upsell();
        let errors = state.has_errors();
        let configured = state.step_configured();
        let last = next_step(state.step).is_none();
        let disabled = upsell.is_some() || errors;
        let back = button(
            "onboarding-notice-back",
            "Back",
            Look::Secondary,
            false,
            disabled,
        )
        .w(px(73.9))
        .flex_row_reverse()
        .child(icon(
            "ob_arrow_left",
            16.,
            "--control-secondary-text-default",
        ))
        .when(!disabled, |this| {
            this.on_click(cx.listener(|this, _, window, cx| {
                this.onboarding_step_button(true, window, cx);
            }))
        });
        let next = (!last).then(|| {
            let look = if configured {
                Look::Primary
            } else {
                Look::Secondary
            };
            let tint = if configured {
                "--white"
            } else {
                "--control-secondary-text-default"
            };
            button(
                "onboarding-notice-next",
                if configured { "Next" } else { "Skip" },
                look,
                false,
                disabled,
            )
            .child(icon("ob_arrow_right", 16., tint))
            .w(px(if configured { 72.8 } else { 70.1 }))
            .when(!disabled, |this| {
                this.on_click(cx.listener(|this, _, window, cx| {
                    this.onboarding_step_button(false, window, cx);
                }))
            })
        });
        let callout = upsell.map(|_| {
            let (number, title, body) = match state.step {
                Step::Safety => (
                    1,
                    "Let's confirm your safety setup",
                    "Discord Raid and DM protection is active. Enable any additional Automod settings now.",
                ),
                Step::Channels => (
                    2,
                    "What are the channels every member should have?",
                    "Choose as many defaults as you need. Members can pick more channels from Customization Questions. And we'll show members any new channels you make.",
                ),
                Step::Questions => (
                    3,
                    "Ask questions to help members pick additional channels and roles.",
                    "Try using questions like \"What do you want to do here?\" or \"What are your hobbies?\" to further customize each member's channel list.",
                ),
                _ => (
                    4,
                    "Set up your Server Guide",
                    "Try making sure all your Get Started items are actionable. We've added a \"Read the Rules\" task so you don't have to!",
                ),
            };
            div()
                .id("onboarding-upsell")
                .occlude()
                .absolute()
                .left(px(0.))
                .bottom(px(64.))
                .w(px(400.))
                .rounded(px(8.))
                .bg(color("--background-surface-high"))
                .shadow(shadow_high())
                .p(px(16.))
                .flex()
                .flex_col()
                .child(text(14., 18.67, 500., "--text-default").child(format!("Step {number}")))
                .child(div().mt(px(8.)).child(heading_lg(title)))
                .child(div().mt(px(16.)).child(body_sm(body)))
                .child(
                    div().mt(px(16.)).w(px(100.)).child(
                        button("onboarding-upsell-got-it", "Got it!", Look::Primary, true, false)
                            .w(px(100.))
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(state) = this.onboarding_mut() {
                                    let step = state.step;
                                    if let Some(name) = state.upsell_name_for(step) {
                                        state.upsells_dismissed.insert(name);
                                    }
                                }
                                // The button that had focus is gone with it.
                                window.blur(cx);
                                cx.notify();
                            })),
                    ),
                )
                .child(
                    // `educationUpsellArrow_Mzv_GO`: an 8px triangle, 54 from the left, under the box.
                    div()
                        .absolute()
                        .left(px(54.))
                        .bottom(px(-8.))
                        .child(icon("tooltip_caret_fill", 16., "--background-surface-high")),
                )
        });
        Some(
            div()
                .absolute()
                .left_0()
                .bottom_0()
                .w(px(740.))
                .px(px(20.))
                .pb(px(20.))
                .child(
                    div()
                        .id("onboarding-notice")
                        .occlude()
                        .relative()
                        .h(px(56.))
                        .p(px(12.))
                        .rounded(px(8.))
                        .bg(color("--background-surface-high"))
                        .shadow(shadow_high())
                        .flex()
                        .items_center()
                        .justify_between()
                        .children(callout)
                        .child(if upsell.is_some() {
                            // `hidden_Mzv_GO`: it keeps its room but does not show.
                            div().opacity(0.).child(back).into_any_element()
                        } else {
                            back.into_any_element()
                        })
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .when(errors, |this| {
                                    this.child(
                                        text(14., 18., 400., "--text-feedback-warning")
                                            .child("You have incomplete questions"),
                                    )
                                })
                                .children(next),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Module 997211's `z`.
    fn onboarding_landing(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let art = self.images.get(LANDING_ART).cloned();
        div()
            .flex()
            .flex_col()
            .gap(px(32.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .w(px(288.))
                            .h(px(192.))
                            .children(art.map(|art| img(art).w(px(288.)).h(px(192.)))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                text(24., 32., 600., "--text-strong")
                                    .child("Welcome your new members with Onboarding!"),
                            )
                            .child(text(16., 20., 400., "--text-default").child(
                                "Customize channels and roles for your new members. Give them the perfect starting experience.",
                            )),
                    )
                    .child(
                        button("onboarding-check-it-out", "Check it out", Look::Primary, true, false)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.onboarding_go(Step::Safety, Some(window), cx);
                            })),
                    ),
            )
            .child(rule())
            .child(self.onboarding_testimonials(cx))
            .child(rule())
            .child(self.onboarding_values(guild))
            .into_any_element()
    }

    /// `x` and `j` in module 997211.
    fn onboarding_testimonials(&self, cx: &mut Context<Self>) -> AnyElement {
        let index = self.onboarding().map_or(0, |state| state.testimonial) % TESTIMONIALS.len();
        let (before, strong, after, who, avatar) = TESTIMONIALS[index];
        let message = format!("{before}{strong}{after}");
        let highlight = HighlightStyle {
            color: Some(super::hsla("--interactive-text-default")),
            font_weight: Some(FontWeight(600.)),
            ..HighlightStyle::default()
        };
        let range = before.len()..before.len() + strong.len();
        let picture = self.images.get(avatar).cloned();
        let turn = |this: &mut Self, step: isize, cx: &mut Context<Self>| {
            if let Some(state) = this.onboarding_mut() {
                let count = TESTIMONIALS.len() as isize;
                state.testimonial = (state.testimonial as isize + step).rem_euclid(count) as usize;
            }
            cx.notify();
        };
        div()
            .relative()
            .w_full()
            .min_h(px(232.))
            .rounded(px(12.))
            .bg(color("--background-mod-subtle"))
            .overflow_hidden()
            .pt(px(32.))
            .pb(px(40.))
            .px(px(40.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .w(px(580.))
                    .p(px(32.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(
                                StyledText::new(message).with_highlights(vec![(range, highlight)]),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .size(px(16.))
                                    .rounded_full()
                                    .overflow_hidden()
                                    // `avatar_XpIUHy:before`, seen when the picture is gone.
                                    .bg(color("--background-mod-subtle"))
                                    .children(
                                        picture.map(|picture| {
                                            img(picture).size(px(16.)).rounded_full()
                                        }),
                                    ),
                            )
                            .child(body_sm_medium(who)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(24.))
                    .top(px(24.))
                    .text_size(px(64.))
                    .line_height(px(76.8))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--background-base-lowest"))
                    .child("\u{201C}"),
            )
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .right(px(0.))
                    .bottom(px(40.))
                    .flex()
                    .justify_center()
                    .gap(px(16.))
                    .child(
                        icon_button(
                            "onboarding-testimonial-back",
                            "ob_arrow_left",
                            Look::Secondary,
                            32.,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| turn(this, -1, cx))),
                    )
                    .child(
                        icon_button(
                            "onboarding-testimonial-next",
                            "ob_arrow_right",
                            Look::Secondary,
                            32.,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| turn(this, 1, cx))),
                    ),
            )
            .into_any_element()
    }

    /// The three rows at the bottom of module 997211's `z`.
    fn onboarding_values(&self, guild: Id) -> AnyElement {
        let value = |title: &'static str, body: &'static str| {
            div()
                .w(px(313.2))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(heading_sm(title))
                // GPUI's text runs a little wider than Chromium's: the same lines need a few more pixels.
                .child(body_sm(body).w(px(320.)))
        };
        let preview = || {
            div()
                .w(px(314.8))
                .flex_none()
                .relative()
                .rounded(px(8.))
                .bg(color("--background-base-lower"))
                .p(px(16.))
                .flex()
                .flex_col()
                .gap(px(12.))
        };
        let row = || div().w_full().flex().items_center().gap(px(32.));
        let answers = [
            ("\u{1F9D9}", "Discuss cool books", true),
            ("\u{1F3A4}", "Join the weekly book club", false),
            ("\u{1F440}", "Keep up with book news", false),
        ];
        let mut list = div().flex().flex_col().gap(px(8.));
        for (emoji, title, selected) in answers {
            let picture = self.images.get(&format!("emoji:{emoji}")).cloned();
            list = list.child(
                div()
                    .relative()
                    .h(px(34.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color(if selected {
                        "--background-base-lowest"
                    } else {
                        "--background-mod-normal"
                    }))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .w(px(22.))
                            .flex_none()
                            .children(picture.map(|picture| img(picture).size(px(16.)))),
                    )
                    .child(text(14., 18., 500., "--text-strong").child(title))
                    .when(selected, |this| {
                        this.child(
                            div()
                                .absolute()
                                .right(px(-8.))
                                .top(px(-8.))
                                .size(px(20.))
                                .rounded(px(20.))
                                .bg(color("--brand-500"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("ob_check", 16., "--white")),
                        )
                    }),
            );
        }
        let question = preview()
            .child(heading_sm("What do you want to do in this server?"))
            .child(list)
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-default"))
                    .child(
                        StyledText::new("You will be added to @Books and #book-chat channel")
                            .with_highlights(vec![
                                (
                                    21..27,
                                    HighlightStyle {
                                        font_weight: Some(FontWeight(600.)),
                                        ..HighlightStyle::default()
                                    },
                                ),
                                (
                                    32..42,
                                    HighlightStyle {
                                        font_weight: Some(FontWeight(600.)),
                                        ..HighlightStyle::default()
                                    },
                                ),
                            ]),
                    ),
            );
        let me = self
            .stores
            .current_user
            .as_ref()
            .map(|user| user.id.to_string());
        let welcome = Welcome {
            author_ids: me.into_iter().collect(),
            message: LANDING_WELCOME.to_owned(),
        };
        let task = |title: &'static str, place: &'static str| {
            div()
                .rounded(px(8.))
                .bg(color("--background-surface-high"))
                .py(px(12.))
                .px(px(20.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .size(px(40.))
                        .flex_none()
                        .rounded(px(20.))
                        .bg(color("--background-base-lowest"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("ob_text", 20., "--icon-muted")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(text(16., 20., 600., "--text-strong").child(title))
                        .child(text(12., 16., 400., "--text-muted").child(place)),
                )
                .child(
                    div()
                        .relative()
                        .size(px(24.))
                        .flex_none()
                        // `secondaryColor: "#fff"`: the disc under the cut-out check.
                        .child(
                            div()
                                .absolute()
                                .left(px(2.))
                                .top(px(2.))
                                .size(px(20.))
                                .rounded_full()
                                .bg(color("--white")),
                        )
                        .child(div().absolute().child(icon(
                            "ob_circle_check",
                            24.,
                            "--status-positive",
                        ))),
                )
        };
        let guide = preview()
            .child(self.onboarding_welcome_card(guild, &welcome))
            .child(task("Introduce yourself", "in #introductions"))
            .child(task("Share your favorite thing", "in #showcase"));
        let picks = [
            ("ob_announce", 16., "announcements", true),
            ("ob_text", 16., "introductions", true),
            ("ob_text", 16., "bot-spam", false),
            ("ob_forum", 16., "book-recs", true),
        ];
        let mut channels = preview();
        for (index, (glyph, size, name, selected)) in picks.into_iter().enumerate() {
            channels = channels.child(
                div()
                    .h(px(if index == 0 { 36. } else { 40. }))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color(if selected {
                        "--background-base-lowest"
                    } else {
                        "--background-mod-normal"
                    }))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon(glyph, size, "--text-strong"))
                            .child(text(16., 20., 500., "--text-strong").child(name)),
                    )
                    .child(
                        div()
                            .size(px(20.))
                            .rounded(px(4.))
                            .border_2()
                            .border_color(color(if selected {
                                "--brand-500"
                            } else {
                                "--interactive-text-default"
                            }))
                            .when(selected, |this| this.bg(color("--brand-500")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(selected, |this| {
                                this.child(icon("ob_check", 16., "--white"))
                            }),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(32.))
            .child(row().child(question).child(value(
                "Help members pick additional channels and roles",
                "Create customization questions and ask members to answer them before or after joining. Hassle-free and easy to use, for both you and your members!",
            )))
            .child(row().child(value(
                "Say hi to every member and help them engage",
                "Create a custom welcome message and instructions for getting started. You can also show members resource pages for FAQ, Rules, and more.",
            )).child(guide))
            .child(row().child(channels).child(value(
                "Choose which channels new members start with",
                "Give them a set of channels that's just the stuff they care about. Hide any unwanted Archives or bot-spam. Less overwhelming, more chatting!",
            )))
            .into_any_element()
    }

    /// Module 913423 (`gC`): the 48 avatar sits half above the card.
    fn onboarding_welcome_card(&self, guild: Id, welcome: &Welcome) -> AnyElement {
        let author = welcome
            .author_ids
            .first()
            .and_then(|id| id.parse::<Id>().ok())
            .and_then(|id| {
                if self
                    .stores
                    .current_user
                    .as_ref()
                    .is_some_and(|me| me.id == id)
                {
                    self.stores.current_user.clone()
                } else {
                    self.stores.users.get(&id).cloned()
                }
            });
        let Some(author) = author else {
            return div().into_any_element();
        };
        let viewer = self
            .stores
            .current_user
            .as_ref()
            .map_or(String::new(), |user| user.display_name().to_owned());
        let owner = self
            .stores
            .guild(guild)
            .is_some_and(|known| known.owner_id == Some(author.id));
        let picture = self.images.get(&avatar_source(&author).0).cloned();
        // `l.split(/\[@username\]/g)`.
        let mention = format!("@{viewer}");
        let mut message = String::new();
        let mut ranges = Vec::new();
        for (index, part) in welcome.message.split("[@username]").enumerate() {
            if index > 0 {
                ranges.push((
                    message.len()..message.len() + mention.len(),
                    HighlightStyle {
                        font_weight: Some(FontWeight(600.)),
                        ..HighlightStyle::default()
                    },
                ));
                message.push_str(&mention);
            }
            message.push_str(part);
        }
        div()
            .relative()
            .rounded(px(8.))
            .bg(color("--background-surface-high"))
            .child(
                div()
                    .pt(px(4.))
                    .px(px(16.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(18.))
                            .pl(px(58.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                text(14., 18., 600., "--text-strong")
                                    .child(author.display_name().to_owned()),
                            )
                            .when(owner, |this| {
                                this.child(
                                    gpui::svg()
                                        .path("icons/owner_crown.svg")
                                        .size(px(12.))
                                        .text_color(CROWN),
                                )
                            }),
                    )
                    .child(
                        div()
                            .mt(px(10.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(StyledText::new(message).with_highlights(ranges)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(20.))
                    .top(px(-20.))
                    .size(px(48.))
                    .rounded_full()
                    .bg(color("--background-base-low"))
                    .overflow_hidden()
                    .children(picture.map(|picture| img(picture).size(px(48.)).rounded_full())),
            )
            .into_any_element()
    }

    /// `mA`: the link that asks first.
    fn onboarding_mode_link(&self, cx: &mut Context<Self>) -> AnyElement {
        let advanced = self.onboarding().is_some_and(|state| state.saved.mode == 1);
        link(if advanced {
            "Switch to Regular Mode"
        } else {
            "Switch to Advanced Mode"
        })
        .id("onboarding-mode-link")
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(state) = this.onboarding_mut() {
                state.confirm = Some(Confirm::Mode {
                    advanced: !advanced,
                });
            }
            cx.notify();
        }))
        .into_any_element()
    }

    /// `m7`.
    fn onboarding_channels(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.onboarding() else {
            return div().into_any_element();
        };
        let collapsed = state.channels_collapsed;
        let advanced = state.advanced;
        div()
            .w(px(660.))
            .pt(px(32.))
            .flex()
            .items_start()
            .gap(px(20.))
            .child(
                div()
                    .relative()
                    .w(px(400.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("onboarding-channels-collapse")
                            .absolute()
                            .right(px(8.))
                            .top(px(8.))
                            .cursor_pointer()
                            .child(if collapsed {
                                gpui::svg()
                                    .path("icons/ob_caret.svg")
                                    .size(px(16.))
                                    .text_color(color("--interactive-text-default"))
                                    .with_transformation(gpui::Transformation::rotate(
                                        gpui::percentage(0.5),
                                    ))
                                    .into_any_element()
                            } else {
                                icon("ob_caret", 16., "--interactive-text-default")
                                    .into_any_element()
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(state) = this.onboarding_mut() {
                                    state.channels_collapsed = !state.channels_collapsed;
                                }
                                cx.notify();
                            })),
                    )
                    .child(heading_lg("Default Channels"))
                    .child(div().mt(px(8.)).child(body_sm(
                        "Set default channels every member will see in their channel list when they join. Members can add more channels using Customization Questions.",
                    )))
                    .child(div().mt(px(8.)).child(self.onboarding_mode_link(cx)))
                    .when(!collapsed, |this| {
                        this.children(self.onboarding_recommended(guild, cx))
                            .child(self.onboarding_browser(guild, cx))
                    })
                    .when(advanced, |this| {
                        this.child(div().mt(px(24.)).child(rule()))
                            .child(div().mt(px(40.)).child(heading_lg("Pre-join Questions")))
                            .child(body_sm(
                                "Set questions to ask every member on join. Answers will be used to assign roles and channels.",
                            ))
                            .child(
                                div()
                                    .mt(px(16.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(16.))
                                    .child(self.onboarding_prompt_list(guild, true, false, cx)),
                            )
                    }),
            )
            .child(self.onboarding_preview_column(guild))
            .into_any_element()
    }

    /// `m9`.
    fn onboarding_recommended(&self, guild: Id, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.onboarding()?;
        if state.saved.enabled || state.recommended_dismissed || state.top_channels.is_empty() {
            return None;
        }
        let known = self.stores.guild(guild)?;
        let rules = self.onboarding_rules_channel(guild);
        let rows: Vec<Channel> = recommended(known, &state.top_channels, rules)
            .into_iter()
            .filter(|channel| !state.channels.contains(&channel.id.to_string()))
            .collect();
        if rows.is_empty() {
            return None;
        }
        let last = rows.len() - 1;
        let mut list = div().mt(px(16.)).rounded(px(8.)).overflow_hidden();
        for (index, channel) in rows.iter().enumerate() {
            let id = channel.id;
            let open = can_default(known, channel);
            list = list.child(
                div()
                    .min_h(px(36.))
                    .py(px(12.))
                    .px(px(16.))
                    .bg(color("--background-base-lower"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(icon(
                                        channel_glyph(known, channel, rules),
                                        20.,
                                        "--icon-muted",
                                    ))
                                    .child(
                                        text(16., 20., 500., "--text-default")
                                            .child(channel.name.clone()),
                                    ),
                            )
                            .children(channel.topic.clone().filter(|topic| !topic.is_empty()).map(
                                |topic| {
                                    text(12., 16., 400., "--text-muted").truncate().child(topic)
                                },
                            )),
                    )
                    .child(if open {
                        button(
                            SharedString::from(format!("onboarding-recommended-add-{id}")),
                            "Add",
                            Look::Primary,
                            false,
                            false,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.onboarding_row_clicked(guild, id, cx);
                        }))
                    } else {
                        button(
                            SharedString::from(format!("onboarding-recommended-unlock-{id}")),
                            "Unlock",
                            Look::Secondary,
                            false,
                            false,
                        )
                        .flex_row_reverse()
                        .child(icon("ob_lock", 16., "--control-secondary-text-default"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.onboarding_open_unlock(guild, id);
                            cx.notify();
                        }))
                    }),
            );
            if index != last {
                list = list.child(div().mx(px(16.)).h(px(1.)).bg(color("--border-subtle")));
            }
        }
        Some(
            div()
                .mt(px(24.))
                .flex()
                .flex_col()
                .child(text(16., 20., 500., "--text-strong").child("Recommended"))
                .child(
                    div()
                        .mt(px(4.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            text(12., 16., 400., "--text-muted")
                                .child("These are the channels your members read most."),
                        )
                        .child(
                            div()
                                .id("onboarding-recommended-dismiss")
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(icon("ob_check", 12., "--text-brand"))
                                .child(text(12., 16., 500., "--text-brand").child("Dismiss all"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(state) = this.onboarding_mut() {
                                        state.recommended_dismissed = true;
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .child(list)
                .child(div().mt(px(24.)).child(rule()))
                .into_any_element(),
        )
    }

    /// The guild's rules channel as `GET /guilds/{id}` said; the state store does not keep it.
    fn onboarding_rules_channel(&self, guild: Id) -> Option<Id> {
        self.fetched
            .get(&format!("{ONBOARDING_PREFIX}guild:{guild}"))?
            .get("body")?
            .get("rules_channel_id")?
            .as_str()?
            .parse()
            .ok()
    }

    /// `my`.
    fn onboarding_browser(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some((state, known)) = self.onboarding().zip(self.stores.guild(guild)) else {
            return div().into_any_element();
        };
        let rules = self.onboarding_rules_channel(guild);
        let mut list = div().flex().flex_col();
        for (group_index, group) in browser_groups(known).into_iter().enumerate() {
            // `mS`.
            list = list.child(match group.category {
                None => div()
                    .h(px(26.67))
                    .pb(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.67))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-muted"))
                    .child("Uncategorized")
                    .into_any_element(),
                Some(category) => {
                    let id = category.id;
                    let checked = state.channels.contains(&id.to_string());
                    div()
                        .id(SharedString::from(format!("onboarding-category-{id}")))
                        .h(px(52.))
                        .pt(px(24.))
                        .pr(px(16.))
                        .pb(px(8.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            text(14., 18.67, 500., "--text-muted")
                                .truncate()
                                .child(category.name.clone()),
                        )
                        .child(checkbox(checked, false))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.onboarding_row_clicked(guild, id, cx);
                        }))
                        .into_any_element()
                }
            });
            let last = group.channels.len().saturating_sub(1);
            let category_id = group.category.map(|category| category.id.to_string());
            let followed = category_id
                .as_ref()
                .is_some_and(|id| state.channels.contains(id));
            for (index, channel) in group.channels.iter().enumerate() {
                let id = channel.id;
                let open = can_default(known, channel);
                let checked = followed || state.channels.contains(&id.to_string());
                let tip = if followed {
                    Some("You must unfollow this category to add or remove specific channels.")
                } else if !everyone_can(known, channel, VIEW) {
                    Some("@everyone must have View permission for this channel.")
                } else if group
                    .category
                    .is_some_and(|category| Some(category.id) == rules)
                    || Some(id) == rules
                {
                    Some("We don't recommend adding the rules channel as a Default Channel")
                } else if is_vocal(channel) {
                    Some(
                        "Voice Channels you don't select will still be available in the Voice Channel section of the channel list.",
                    )
                } else {
                    None
                };
                let popular = state.top_channels.contains(&id.to_string());
                let row = div()
                    .id(SharedString::from(format!(
                        "onboarding-channel-{group_index}-{id}"
                    )))
                    .w(px(400.))
                    .min_h(px(36.))
                    .py(px(12.))
                    .px(px(16.))
                    .bg(color("--background-base-lower"))
                    .when(index == 0, |this| this.rounded_t(px(8.)))
                    .when(index == last, |this| this.rounded_b(px(8.)))
                    .when(!followed, |this| {
                        this.cursor_pointer()
                            .hover(|style| style.bg(color("--background-mod-normal")))
                    })
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(icon(
                                        channel_glyph(known, channel, rules),
                                        20.,
                                        "--icon-muted",
                                    ))
                                    .child(
                                        text(
                                            16.,
                                            20.,
                                            500.,
                                            if followed {
                                                "--text-muted"
                                            } else {
                                                "--text-default"
                                            },
                                        )
                                        .truncate()
                                        .child(channel.name.clone()),
                                    ),
                            )
                            .when(popular, |this| {
                                this.child(text(12., 16., 400., "--text-muted").child("\u{B7}"))
                                    .child(text(12., 16., 500., "--text-brand").child("Popular"))
                            }),
                    )
                    .child(if open {
                        checkbox(checked, followed).into_any_element()
                    } else {
                        button(
                            SharedString::from(format!("onboarding-unlock-{id}")),
                            "Unlock",
                            Look::Secondary,
                            false,
                            false,
                        )
                        .w(px(86.1))
                        .flex_row_reverse()
                        .child(icon("ob_lock", 16., "--control-secondary-text-default"))
                        .into_any_element()
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !followed {
                            this.onboarding_row_clicked(guild, id, cx);
                        }
                    }));
                list = list.child(match tip {
                    Some(tip) => self
                        .with_tip(
                            format!("onboarding-row-{group_index}-{id}"),
                            Tip::text(tip, Side::Top),
                            row,
                            cx,
                        )
                        .into_any_element(),
                    None => row.into_any_element(),
                });
                if index != last {
                    list = list.child(
                        div()
                            .mt(px(-1.))
                            .mx(px(16.))
                            .h(px(1.))
                            .bg(color("--border-subtle")),
                    );
                }
            }
        }
        div().mt(px(16.)).child(list).into_any_element()
    }

    /// `mM`.
    fn onboarding_preview_column(&self, guild: Id) -> AnyElement {
        let Some((state, known)) = self.onboarding().zip(self.stores.guild(guild)) else {
            return div().into_any_element();
        };
        let ids: Vec<String> = state.channels.iter().cloned().collect();
        let (chattable, all) = default_reach(known, &ids);
        let advanced = state.saved.mode == 1;
        let required_all = required_reach(known, &ids, &state.prompts, false);
        let required_chat = required_reach(known, &ids, &state.prompts, true);
        let shown_all = if advanced {
            required_all.len()
        } else {
            all.len()
        };
        let shown_chat = if advanced {
            required_chat.len()
        } else {
            chattable.len()
        };
        let extra = required_all.len().saturating_sub(all.len());
        let rows = sidebar_rows(known, &state.channels);
        let rules = self.onboarding_rules_channel(guild);
        let note = |content: AnyElement| {
            div()
                .mt(px(12.))
                .flex()
                .gap(px(8.))
                .child(icon("circle_info", 18., "--text-muted"))
                // A little over its 182: GPUI's text runs wider than Chromium's.
                .child(div().w(px(190.)).flex_none().child(content))
        };
        let mut list = div().flex().flex_col();
        for channel in &rows {
            if channel.kind == ChannelKind::Category {
                list = list.child(
                    div()
                        .mt(px(16.))
                        .h(px(16.))
                        .pl(px(4.))
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .child(icon("automod_chevron_down", 12., "--text-muted"))
                        .child(text(12., 16., 600., "--text-muted").child(channel.name.clone())),
                );
            } else {
                list = list.child(
                    div()
                        .mt(px(4.))
                        .h(px(28.))
                        .px(px(18.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon(
                            channel_glyph(known, channel, rules),
                            16.,
                            "--text-muted",
                        ))
                        .child(
                            text(16., 20., 400., "--text-muted")
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .child(channel.name.clone()),
                        )
                        .when(is_chattable(known, channel), |this| {
                            this.child(
                                div()
                                    .size(px(8.))
                                    .flex_none()
                                    .rounded_full()
                                    .bg(color("--text-brand")),
                            )
                        }),
                );
            }
        }
        let empty = rows.is_empty();
        div()
            .w(px(240.))
            .flex_none()
            .rounded(px(12.))
            .overflow_hidden()
            .bg(color("--background-base-lower"))
            .flex()
            .flex_col()
            .child(
                div()
                    .pt(px(16.))
                    .px(px(16.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(16.))
                            .flex()
                            .gap(px(8.))
                            .child(
                                text(12., 16., 600., "--text-brand")
                                    .child(format!("{shown_chat} chattable")),
                            )
                            .child(
                                text(12., 16., 600., "--text-muted")
                                    .child(format!("{shown_all} total")),
                            ),
                    )
                    .when(advanced, |this| {
                        this.child(note(
                            text(12., 16., 400., "--text-muted")
                                .child("Total is calculated by the number of Default Channels + minimum possible channels assigned through Required Questions")
                                .into_any_element(),
                        ))
                        .child(note(
                            text(12., 16., 400., "--text-muted")
                                .child(StyledText::new(format!(
                                    "Minimum {extra} channels assigned through required Pre-join Questions."
                                )).with_highlights(vec![(
                                    format!("Minimum {extra} channels assigned through required ").len()
                                        ..format!("Minimum {extra} channels assigned through required Pre-join Questions").len(),
                                    HighlightStyle {
                                        color: Some(super::hsla("--text-link")),
                                        ..HighlightStyle::default()
                                    },
                                )]))
                                .into_any_element(),
                        ))
                    }),
            )
            .child(div().mt(px(if advanced { 16. } else { 24. })).child(rule()))
            .child(if empty {
                div()
                    .pt(px(24.))
                    .pb(px(32.))
                    .px(px(16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(40.))
                            .rounded_full()
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("ob_text", 24., "--text-muted")),
                    )
                    .child(
                        text(16., 20., 400., "--text-muted")
                            .text_center()
                            .child("Add some channels that all users will see!"),
                    )
                    .into_any_element()
            } else {
                div().pb(px(16.)).child(list).into_any_element()
            })
            .into_any_element()
    }

    /// `g9`.
    fn onboarding_questions(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.onboarding() else {
            return div().into_any_element();
        };
        let advanced = state.advanced;
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(heading_lg(if advanced {
                "Post-join Questions"
            } else {
                "Questions"
            }))
            .child(div().mt(px(8.)).child(body_sm(
                "Create questions to help members pick additional channels and roles. Their channel list will be customized based on their answers.",
            )))
            .child(
                div()
                    .mt(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        link("See examples")
                            .id("onboarding-examples")
                            .on_click(cx.listener(|_, _, _, cx| cx.open_url(EXAMPLES))),
                    )
                    .child(link_dot())
                    .child(
                        // `dm(t)` and then `X$`: the save happens, the preview does not exist yet.
                        link("Preview")
                            .id("onboarding-preview-link")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.onboarding_save_prompts(false, After::Nothing, cx);
                            })),
                    )
                    .child(link_dot())
                    .child(self.onboarding_mode_link(cx)),
            )
            .child(self.onboarding_coverage(guild, cx))
            .child(
                div()
                    .mt(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(self.onboarding_prompt_list(guild, false, advanced, cx)),
            )
            .into_any_element()
    }

    /// `g6`.
    fn onboarding_coverage(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some((state, known)) = self.onboarding().zip(self.stores.guild(guild)) else {
            return div().into_any_element();
        };
        let ids: Vec<String> = state.channels.iter().cloned().collect();
        let (covered, missing) = coverage(known, &ids, &state.prompts);
        let total = covered.len() + missing.len();
        let percent = if total == 0 {
            0.
        } else {
            covered.len() as f32 / total as f32 * 100.
        };
        let needed = ((85 * total).div_ceil(100)).saturating_sub(covered.len());
        let open = state.coverage_open;
        let rules = self.onboarding_rules_channel(guild);
        let count = if missing.is_empty() {
            "No".to_owned()
        } else {
            missing.len().to_string()
        };
        div()
            .mt(px(24.))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(color("--background-base-lowest"))
            .flex()
            .flex_col()
            .child(
                div()
                    .id("onboarding-coverage-toggle")
                    .h(px(48.))
                    .px(px(16.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .when(percent <= 85., |this| {
                                this.child(icon("ob_warning", 16., "--status-warning"))
                            })
                            .child(text(12., 16., 500., "--text-muted").child(format!(
                                "{count} public channels are missing from Questions and Default Channels."
                            ))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                // `g3.i`: 6 tall, `--background-mod-strong`, the fill's colour by the percentage.
                                div()
                                    .relative()
                                    .w(px(120.))
                                    .h(px(6.))
                                    .rounded(px(3.))
                                    .overflow_hidden()
                                    .bg(color("--background-mod-strong"))
                                    .child(
                                        div()
                                            .absolute()
                                            .left_0()
                                            .top_0()
                                            .h(px(6.))
                                            .w(px(120. * percent / 100.))
                                            .rounded(px(3.))
                                            .bg(color(if percent > 85. {
                                                "--status-positive"
                                            } else {
                                                "--status-warning"
                                            })),
                                    ),
                            )
                            .child(if !open {
                                gpui::svg()
                                    .path("icons/ob_chevron_up.svg")
                                    .size(px(24.))
                                    .text_color(color("--interactive-text-default"))
                                    .with_transformation(gpui::Transformation::rotate(
                                        gpui::percentage(0.5),
                                    ))
                                    .into_any_element()
                            } else {
                                icon("ob_chevron_up", 24., "--interactive-text-default")
                                    .into_any_element()
                            }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this.onboarding_mut() {
                            state.coverage_open = !state.coverage_open;
                        }
                        cx.notify();
                    })),
            )
            .when(open, |this| {
                let mut pills = div().mt(px(8.)).flex().flex_wrap().gap(px(4.));
                for channel in &missing {
                    pills = pills.child(
                        div()
                            .h(px(20.))
                            .px(px(4.))
                            .rounded(px(2.))
                            .bg(color("--background-mod-subtle"))
                            .flex()
                            .items_center()
                            .child(icon(channel_glyph(known, channel, rules), 12., "--interactive-text-active"))
                            .child(
                                text(12., 16., 400., "--interactive-text-active")
                                    .child(channel.name.clone()),
                            ),
                    );
                }
                this.child(
                    div()
                        .py(px(12.))
                        .px(px(16.))
                        .bg(color("--background-base-lower"))
                        .flex()
                        .flex_col()
                        .child(text(12., 16., 600., "--text-muted").child("CHANNELS NOT ASSIGNED"))
                        .child(pills)
                        .child(
                            div()
                                .mt(px(16.))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .when(needed > 0, |this| {
                                    this.child(icon("ob_warning", 16., "--text-muted")).child(
                                        text(12., 16., 400., "--text-muted").child(format!(
                                            "Add at least {needed} more channels to Questions or Default Channels, or members will have a hard time finding them."
                                        )),
                                    )
                                })
                                .when(needed == 0, |this| {
                                    this.child(
                                        text(12., 16., 400., "--text-muted").child("No channels here"),
                                    )
                                }),
                        ),
                )
            })
            .into_any_element()
    }

    /// `m3`, with `m2` between its two lists.
    fn onboarding_prompt_list(
        &self,
        guild: Id,
        prejoin_only: bool,
        postjoin_only: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.onboarding() else {
            return div().into_any_element();
        };
        let before: Vec<&Prompt> = state.prompts.iter().filter(|p| p.in_onboarding).collect();
        let after: Vec<&Prompt> = state.prompts.iter().filter(|p| !p.in_onboarding).collect();
        let before_count = before.len();
        let total = state.prompts.len();
        let add = |id: &'static str, in_onboarding: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(56.))
                .p(px(16.))
                .rounded(px(12.))
                .border_2()
                .border_dashed()
                .border_color(color("--border-subtle"))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .mr(px(8.))
                        .child(icon("ob_plus_circle", 20., "--text-brand")),
                )
                .child(text(16., 20., 500., "--text-brand").child("Add a Question"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let id = this.onboarding_mut().map_or(String::new(), |state| {
                        state.sequence += 1;
                        local_id(state.sequence)
                    });
                    this.onboarding_edit_prompts(
                        |prompts| prompts.push(Prompt::blank(id, in_onboarding)),
                        false,
                        cx,
                    );
                }))
        };
        let mut list = div().flex().flex_col().gap(px(16.));
        if !postjoin_only {
            if !prejoin_only {
                list = list.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(heading_sm("Pre-join Questions"))
                        .child(div().mt(px(8.)).child(body_sm(
                            "Members will be asked these questions before they join your server. Use them to assign channels and important roles. Pre-join Questions will also be available on the Channels & Roles page.",
                        ))),
                );
            }
            for (index, prompt) in before.iter().enumerate() {
                list = list.child(self.onboarding_prompt_card(
                    guild,
                    prompt,
                    index,
                    index,
                    prejoin_only,
                    cx,
                ));
            }
        }
        // `m2`: the separator is a drop target too, and pre-join's Add sits on it.
        let separator_index = before_count;
        let mut separator = div()
            .id("onboarding-prompts-separator")
            .flex()
            .flex_col()
            .on_drop(cx.listener(move |this, drag: &PromptDrag, _, cx| {
                this.onboarding_prompt_dropped(drag.0, separator_index, cx);
            }));
        if !postjoin_only && before_count < MAX_PREJOIN {
            separator = separator.child(add("onboarding-add-prejoin", true, cx));
        }
        if !prejoin_only {
            separator = separator
                .child(div().mt(px(20.)).child(rule()))
                .child(div().mt(px(20.)).child(heading_sm("Post-join Questions")))
                .child(div().mt(px(8.)).child(body_sm(
                    "Members will be asked these questions after they join your server, on the Channels & Roles page. Use them to\nassign roles that members can pick later, like vanity roles.",
                )));
        }
        list = list.child(separator);
        if !prejoin_only {
            for (index, prompt) in after.iter().enumerate() {
                list = list.child(self.onboarding_prompt_card(
                    guild,
                    prompt,
                    index + before_count,
                    index + before_count + 1,
                    false,
                    cx,
                ));
            }
            if total < MAX_PROMPTS {
                list = list.child(add("onboarding-add-postjoin", false, cx));
            }
        }
        list.into_any_element()
    }

    fn onboarding_prompt_dropped(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding() else {
            return;
        };
        match move_prompt(&state.prompts, from, to) {
            Some(list) => self.onboarding_edit_prompts(|prompts| *prompts = list, true, cx),
            None => {
                self.onboarding_alert(
                    "Cannot move this Question",
                    "You cannot have more than 4 pre-join Questions".to_owned(),
                );
                cx.notify();
            }
        }
    }

    /// `mQ`.
    fn onboarding_prompt_card(
        &self,
        guild: Id,
        prompt: &Prompt,
        index: usize,
        drag_index: usize,
        single_column: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.onboarding() else {
            return div().into_any_element();
        };
        let error = state.errors.get(index).and_then(Option::as_ref);
        let id = prompt.id.clone();
        let group = SharedString::from(format!("onboarding-prompt-{id}"));
        let collapsed = state.collapsed.contains(&id);
        let trash = {
            let id = id.clone();
            self.with_tip(
                format!("onboarding-delete-{id}"),
                Tip::text("Delete this question", Side::Top),
                div()
                    .id(SharedString::from(format!("onboarding-delete-{id}")))
                    .size(px(32.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("ob_trash", 16., "--control-icon-only-icon-default"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let id = id.clone();
                        this.onboarding_edit_prompts(
                            |prompts| prompts.retain(|prompt| prompt.id != id),
                            true,
                            cx,
                        );
                    })),
                cx,
            )
        };
        let collapse = {
            let id = id.clone();
            div()
                .id(SharedString::from(format!("onboarding-collapse-{id}")))
                .absolute()
                .right(px(12.))
                .top(px(12.))
                .cursor_pointer()
                .child(if collapsed {
                    gpui::svg()
                        .path("icons/ob_caret.svg")
                        .size(px(16.))
                        .text_color(color("--interactive-text-default"))
                        .with_transformation(gpui::Transformation::rotate(gpui::percentage(0.5)))
                        .into_any_element()
                } else {
                    icon("ob_caret", 16., "--interactive-text-default").into_any_element()
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this.onboarding_mut()
                        && !state.collapsed.remove(&id)
                    {
                        state.collapsed.insert(id.clone());
                    }
                    cx.notify();
                }))
        };
        let handle = div()
            .id(SharedString::from(format!("onboarding-drag-{id}")))
            .absolute()
            .left_0()
            .right_0()
            .top_0()
            .h(px(24.))
            .invisible()
            .group_hover(group.clone(), |style| style.visible())
            .cursor_grab()
            .flex()
            .items_center()
            .justify_center()
            .child(icon("ob_drag", 16., "--interactive-text-default"))
            .on_drag(PromptDrag(drag_index), |_, _, _, cx| cx.new(|_| DragGhost));
        let base = div()
            .id(group.clone())
            .group(group)
            .relative()
            .w_full()
            .p(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .on_drop(cx.listener(move |this, drag: &PromptDrag, _, cx| {
                this.onboarding_prompt_dropped(drag.0, drag_index, cx);
            }));
        let label = format!("Question {}", index + 1);
        if collapsed {
            return base
                .flex()
                .flex_col()
                .child(handle)
                .child(text(12., 16., 600., "--text-muted").child(label))
                .child(text(18., 22., 600., "--text-default").child(prompt.title.clone()))
                .child(collapse)
                .child(div().absolute().right(px(16.)).bottom(px(8.)).child(trash))
                .into_any_element();
        }
        let field = state.fields.get(&id).map(|field| {
            self.onboarding_input(
                &field.editor,
                error.and_then(|error| error.title).is_some(),
                40.,
                cx,
            )
        });
        let answers_label = if prompt.options.len() >= DROPDOWN_AT {
            format!(
                "Available Answers \u{2014} {} of {MAX_OPTIONS} shown in a dropdown list",
                prompt.options.len()
            )
        } else {
            format!(
                "Available Answers \u{2014} {} of {MAX_OPTIONS}",
                prompt.options.len()
            )
        };
        let mut options = div().flex().flex_wrap().gap(px(8.));
        for (position, answer) in prompt.options.iter().enumerate() {
            options = options.child(self.onboarding_answer_card(
                &id,
                answer,
                position,
                index,
                error.and_then(|error| error.option_errors.get(position).copied().flatten()),
                single_column,
                cx,
            ));
        }
        if prompt.options.len() < MAX_OPTIONS {
            options =
                options.child(self.onboarding_add_answer(&id, prompt, index, single_column, cx));
        }
        let shown_error = error.and_then(|error| {
            error
                .options
                .or_else(|| error.option_errors.iter().flatten().copied().next())
        });
        let examples = prompt.title.is_empty() && !state.examples_shown.contains(&id);
        let required_label = if prompt.required && state.step == Step::Channels {
            let known = self.stores.guild(guild);
            let ids: Vec<String> = state.channels.iter().cloned().collect();
            let count = known.map_or(0, |known| {
                required_reach(known, &ids, std::slice::from_ref(prompt), false)
                    .len()
                    .saturating_sub(ids.len())
            });
            match count {
                0 => "Required".to_owned(),
                1 => "Required + Assigns at least 1 channel".to_owned(),
                count => format!("Required + Assigns at least {count} channels"),
            }
        } else {
            "Required".to_owned()
        };
        let multi_id = id.clone();
        let required_id = id.clone();
        base.flex()
            .flex_col()
            .child(handle)
            .child(collapse)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(text(16., 20., 500., "--text-strong").child(label))
                            .children(field)
                            .children(error.and_then(|error| error.title).map(|message| {
                                text(12., 16., 400., "--text-feedback-critical").child(message)
                            })),
                    )
                    .when(examples, |this| {
                        this.child(self.onboarding_title_examples(&id, cx))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(text(16., 20., 500., "--text-strong").child(answers_label))
                            .child(options)
                            .children(shown_error.map(|message| {
                                text(12., 16., 400., "--text-feedback-critical").child(message)
                            })),
                    ),
            )
            .child(
                div()
                    .mt(px(24.))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(24.))
                            .child(
                                div()
                                    .id(SharedString::from(format!("onboarding-multiple-{id}")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(checkbox(!prompt.single_select, false))
                                    .child(
                                        text(16., 20., 400., "--text-strong")
                                            .child("Allow multiple answers"),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.onboarding_edit_prompt(
                                            &multi_id,
                                            |prompt| prompt.single_select = !prompt.single_select,
                                            cx,
                                        );
                                    })),
                            )
                            .when(prompt.in_onboarding, |this| {
                                this.child(
                                    div()
                                        .id(SharedString::from(format!("onboarding-required-{id}")))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .gap(px(12.))
                                        .child(checkbox(prompt.required, false))
                                        .child(
                                            text(16., 20., 400., "--text-strong")
                                                .child(required_label),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.onboarding_edit_prompt(
                                                &required_id,
                                                |prompt| prompt.required = !prompt.required,
                                                cx,
                                            );
                                        })),
                                )
                            }),
                    )
                    .child(trash),
            )
            .children(error.and_then(|error| error.config.clone()).map(|message| {
                div()
                    .mt(px(8.))
                    .child(text(12., 16., 400., "--text-feedback-critical").child(message))
            }))
            .into_any_element()
    }

    /// `m$`.
    fn onboarding_title_examples(&self, prompt: &str, cx: &mut Context<Self>) -> AnyElement {
        let mut row = div().mt(px(4.)).flex().flex_wrap();
        for (index, example) in [
            "What off-topic channels do you want?",
            "What do you want notifications for?",
            "What kind of events are you interested in?",
            "What\u{2019}s your favorite book genre?",
        ]
        .into_iter()
        .enumerate()
        {
            let id = prompt.to_owned();
            row = row.child(
                div()
                    .id(SharedString::from(format!(
                        "onboarding-example-{prompt}-{index}"
                    )))
                    .h(px(24.))
                    .mb(px(4.))
                    .mr(px(8.))
                    .px(px(8.))
                    .rounded(px(12.))
                    .bg(color("--background-base-lowest"))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-default"))
                    .hover(|style| style.text_color(color("--interactive-text-active")))
                    .child(example)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let id = id.clone();
                        this.onboarding_edit_prompt(
                            &id,
                            |prompt| prompt.title = example.to_owned(),
                            cx,
                        );
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .child(text(12., 16., 400., "--text-muted").child("Here are some examples:"))
            .child(row)
            .into_any_element()
    }

    /// `mX`.
    #[allow(clippy::too_many_arguments)]
    fn onboarding_answer_card(
        &self,
        prompt: &str,
        answer: &Answer,
        position: usize,
        prompt_index: usize,
        error: Option<&'static str>,
        single_column: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bad_emoji = answer.emoji.as_ref().is_some_and(|emoji| {
            !emoji.is_empty() && self.onboarding_emoji_image(emoji).is_none() && emoji.id.is_some()
        });
        let group = SharedString::from(format!("onboarding-answer-{}", answer.id));
        let picture = answer
            .emoji
            .as_ref()
            .and_then(|emoji| self.onboarding_emoji_image(emoji));
        let (prompt_id, answer_id) = (prompt.to_owned(), answer.id.clone());
        let (drop_prompt, drag_prompt) = (prompt.to_owned(), prompt.to_owned());
        div()
            .id(group.clone())
            .group(group.clone())
            .relative()
            .w(px(if single_column { 366. } else { 309. }))
            .flex_none()
            .py(px(12.))
            .px(px(16.))
            .rounded(px(12.))
            .border_2()
            .border_color(if error.is_some() || bad_emoji {
                color("--text-feedback-critical")
            } else {
                color("--border-subtle")
            })
            .hover(|style| style.border_color(color("--border-muted")))
            .cursor_pointer()
            .flex()
            .flex_col()
            .justify_center()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(px(16.))
                    .invisible()
                    .group_hover(group, |style| style.visible())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("ob_drag", 12., "--interactive-text-default")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    // `emoji_qZfI5R` holds its 24 whether or not there is an emoji.
                    .child(
                        div()
                            .mr(px(12.))
                            .w(px(24.))
                            .flex_none()
                            .children(picture.map(|picture| img(picture).size(px(24.)))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                text(16., 20., 500., "--text-strong")
                                    .truncate()
                                    .child(answer.title.clone()),
                            )
                            .when(!answer.description.is_empty(), |this| {
                                this.child(
                                    text(12., 16., 400., "--text-default")
                                        .child(answer.description.clone()),
                                )
                            }),
                    ),
            )
            .when(bad_emoji, |this| {
                this.child(
                    text(12., 16., 500., "--text-feedback-critical")
                        .child("This emoji no longer exists. Select a new one."),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.onboarding_open_answer(&prompt_id, Some(&answer_id), prompt_index, window, cx);
            }))
            .on_drag(AnswerDrag(drag_prompt, position), |_, _, _, cx| {
                cx.new(|_| DragGhost)
            })
            .on_drop(cx.listener(move |this, drag: &AnswerDrag, _, cx| {
                if drag.0 == drop_prompt {
                    let (from, prompt) = (drag.1, drop_prompt.clone());
                    this.onboarding_edit_prompt(
                        &prompt,
                        |prompt| prompt.options = moved(&prompt.options, from, position),
                        cx,
                    );
                }
            }))
            .into_any_element()
    }

    /// `mq`.
    fn onboarding_add_answer(
        &self,
        prompt: &str,
        known: &Prompt,
        index: usize,
        single_column: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = prompt.to_owned();
        let warn = known.options.len() + 1 == DROPDOWN_AT;
        div()
            .id(SharedString::from(format!(
                "onboarding-add-answer-{prompt}"
            )))
            .w(px(if single_column { 366. } else { 309. }))
            .flex_none()
            .h(px(52.))
            .py(px(12.))
            .px(px(16.))
            .rounded(px(12.))
            .border_2()
            .border_dashed()
            .border_color(color("--border-subtle"))
            .hover(|style| style.border_color(color("--border-muted")))
            .cursor_pointer()
            .flex()
            .items_center()
            .child(div().mr(px(12.)).child(icon(
                "ob_plus_circle",
                24.,
                "--interactive-text-default",
            )))
            .child(text(16., 20., 500., "--text-strong").child("Add an Answer"))
            .on_click(cx.listener(move |this, _, window, cx| {
                if warn {
                    if let Some(state) = this.onboarding_mut() {
                        state.confirm = Some(Confirm::Dropdown {
                            prompt: id.clone(),
                            index,
                        });
                    }
                    cx.notify();
                } else {
                    this.onboarding_open_answer(&id, None, index, window, cx);
                }
            }))
            .into_any_element()
    }

    /// `is.k`: the input with its border.
    fn onboarding_input(
        &self,
        editor: &Entity<Editor>,
        error: bool,
        height: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let focused = self
            .onboarding()
            .is_some_and(|state| state.focused == Some(editor.entity_id()));
        let target = editor.clone();
        div()
            .id(SharedString::from(format!(
                "onboarding-input-{}",
                editor.entity_id()
            )))
            .cursor_text()
            .on_click(cx.listener(move |_, _, window, cx| {
                let handle = target.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }))
            .h(px(height))
            .w_full()
            .px(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(if error {
                "--text-feedback-critical"
            } else if focused {
                "--input-border-active"
            } else {
                "--input-border-default"
            }))
            .rule(crate::ui::motion::Rule::TextField)
            .bg(color("--input-background-default"))
            .flex()
            .items_center()
            .child(div().flex_1().min_w(px(0.)).child(editor.clone()))
    }
}

/// A question in the hand (`ONBOARDING_PROMPT_CARD`), by its place among questions and the separator.
#[derive(Clone)]
struct PromptDrag(usize);

/// An answer in the hand (`ONBOARDING_PROMPT_OPTION_CARD-{id}`).
#[derive(Clone)]
struct AnswerDrag(String, usize);

/// `NEW_MEMBER_ACTION`.
#[derive(Clone)]
struct TaskDrag(usize);

/// `RESOURCE_CHANNEL`.
#[derive(Clone)]
struct ResourceDrag(usize);

struct DragGhost;

impl gpui::Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl Root {
    /// `gT`.
    fn onboarding_guide(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.onboarding() else {
            return div().into_any_element();
        };
        let member = state.guide_tab_member;
        let art = self
            .images
            .get(if member { GUIDE_MEMBER } else { GUIDE_JOINER })
            .cloned();
        let tab = |id: &'static str,
                   label: &'static str,
                   on: bool,
                   value: bool,
                   cx: &mut Context<Self>| {
            div()
                .id(id)
                .flex_1()
                .h(px(30.))
                .rounded(px(8.))
                .when(on, |this| this.bg(color("--background-surface-high")))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.))
                .line_height(px(14.))
                .font_weight(FontWeight(500.))
                .text_color(color(if on {
                    "--text-strong"
                } else {
                    "--interactive-text-default"
                }))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this.onboarding_mut() {
                        state.guide_tab_member = value;
                    }
                    cx.notify();
                }))
        };
        let separator = || div().my(px(32.)).child(rule());
        div()
            .w(px(660.))
            .pb(px(48.))
            .flex()
            .items_start()
            .child(
                div()
                    .w(px(396.))
                    .flex_none()
                    .pr(px(32.))
                    .flex()
                    .flex_col()
                    .child(heading_lg("Welcome Sign"))
                    .child(div().mt(px(8.)).child(body_sm_medium(
                        "Tell new members what's special about your community and why you're excited to see them!",
                    )))
                    .child(div().mt(px(24.)).child(self.onboarding_welcome_section(guild, cx)))
                    .child(separator())
                    .child(heading_lg("New Member To Do's"))
                    .child(
                        div().mt(px(8.)).text_size(px(14.)).line_height(px(18.)).font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(StyledText::new(
                                "Optionally add up to 5 tasks for your new members to do. Get them to talk and engage in your channels.",
                            ).with_highlights(vec![(
                                69..102,
                                HighlightStyle { font_weight: Some(FontWeight(600.)), ..HighlightStyle::default() },
                            )])),
                    )
                    .child(
                        div()
                            .mt(px(24.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(body_sm_medium("Don't Do This: Too General"))
                            .child(
                                div()
                                    .py(px(8.))
                                    .px(px(12.))
                                    .rounded(px(12.))
                                    .border_2()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        // `badExampleIcon_nhgNJ0`: 48 by the 51 its line box makes it.
                                        div()
                                            .w(px(48.))
                                            .h(px(51.))
                                            .flex_none()
                                            .rounded_full()
                                            .bg(color("--background-base-lower"))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("ob_x", 24., "--red-400")),
                                    )
                                    .child(
                                        div()
                                            .child(text(16., 20., 600., "--text-strong").child("chat with the community"))
                                            .child(text(12., 16., 500., "--text-muted").child("in #general")),
                                    ),
                            ),
                    )
                    .child(div().mt(px(32.)).child(self.onboarding_tasks(guild, cx)))
                    .child(separator())
                    .child(heading_lg("Resource Pages"))
                    .child(div().mt(px(8.)).child(body_sm_medium(
                        "Turn read-only channels into fancy resource pages in your Server Guide. They will no longer appear on the channel list unless you enable All Channels. Resources come with some perks:",
                    )))
                    .child(
                        div()
                            .mt(px(8.))
                            .pl(px(16.))
                            .flex()
                            .flex_col()
                            .children(
                                [
                                    "Members start at the top of pages instead of the bottom of a message thread",
                                    "Chat bars and avatars are removed so it looks cleaner",
                                    "All the content, embeds, media, and formatting will stay the same",
                                ]
                                .into_iter()
                                .map(|line| {
                                    div()
                                        .relative()
                                        .child(
                                            div()
                                                .absolute()
                                                .left(px(-12.))
                                                .top(px(0.))
                                                .text_size(px(14.))
                                                .line_height(px(18.))
                                                .text_color(color("--text-default"))
                                                .child("\u{2022}"),
                                        )
                                        .child(body_sm_medium(line))
                                }),
                            ),
                    )
                    .child(div().mt(px(24.)).child(self.onboarding_resources(guild, cx)))
                    .child(separator())
                    .child(self.onboarding_banner(guild, cx)),
            )
            .child(
                div()
                    .w(px(264.))
                    .flex_none()
                    .pt(px(8.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(body_sm_medium("Example"))
                    .child(
                        div()
                            .mt(px(10.))
                            .w(px(264.))
                            .h(px(571.6))
                            .children(art.map(|art| img(art).w(px(264.)).h(px(571.6)))),
                    )
                    .child(
                        div()
                            .mt(px(3.))
                            .w(px(264.))
                            .h(px(38.))
                            .p(px(4.))
                            .rounded(px(12.))
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .gap(px(4.))
                            .child(tab("onboarding-tab-joiner", "Joiner", !member, false, cx))
                            .child(tab("onboarding-tab-member", "Member", member, true, cx)),
                    ),
            )
            .into_any_element()
    }

    /// `gv`.
    fn onboarding_welcome_section(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.onboarding() else {
            return div().into_any_element();
        };
        if welcome_is_empty(state.home.welcome.as_ref()) {
            return div()
                .child(
                    button(
                        "onboarding-setup-welcome",
                        "Set Up Welcome Sign",
                        Look::Primary,
                        false,
                        false,
                    )
                    .w(px(152.))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.onboarding_open_welcome(window, cx);
                    })),
                )
                .into_any_element();
        }
        let mut welcome = state.home.welcome.clone().unwrap_or_default();
        if welcome.author_ids.is_empty() {
            welcome.author_ids = self
                .stores
                .current_user
                .iter()
                .map(|user| user.id.to_string())
                .collect();
        }
        div()
            .id("onboarding-welcome-card")
            .group("onboarding-welcome-card")
            // The avatar stands 20 above the card, inside the section's 16 more.
            .mt(px(16.))
            .relative()
            .cursor_pointer()
            .child(self.onboarding_welcome_card(guild, &welcome))
            .child(
                div()
                    .absolute()
                    .right(px(16.))
                    .top(px(12.))
                    .p(px(4.))
                    .rounded(px(4.))
                    .bg(color("--brand-500"))
                    .opacity(0.5)
                    .group_hover("onboarding-welcome-card", |style| style.opacity(1.))
                    .child(icon("ob_pencil", 24., "--white")),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.onboarding_open_welcome(window, cx);
            }))
            .into_any_element()
    }

    /// `go`: the tasks, Read the rules, Add a task.
    fn onboarding_tasks(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some((state, known)) = self.onboarding().zip(self.stores.guild(guild)) else {
            return div().into_any_element();
        };
        let tasks = state.home.tasks.clone().unwrap_or_default();
        let count = tasks.len();
        let mut list = div().flex().flex_col();
        for (index, task) in tasks.iter().enumerate() {
            let channel = known
                .channels
                .iter()
                .find(|channel| channel.id.to_string() == task.channel_id);
            let Some(channel) = channel else { continue };
            let pending = state
                .pending_icons
                .get(&task.channel_id)
                .and_then(|(icon, _)| icon.clone());
            let bad_emoji = task.emoji.as_ref().is_some_and(|emoji| {
                emoji.id.is_some() && self.onboarding_emoji_image(emoji).is_none()
            });
            let error = if !task_channel(known, channel) {
                Some("Assigned channel is missing permissions for @everyone")
            } else if bad_emoji {
                Some("This emoji no longer exists. Select a new one.")
            } else {
                None
            };
            let picture = pending
                .as_ref()
                .and_then(|uri| self.onboarding_data_image(uri))
                .or_else(|| {
                    Self::task_icon_url(task).and_then(|url| self.images.get(&url).cloned())
                });
            let emoji_picture = task
                .emoji
                .as_ref()
                .and_then(|emoji| self.onboarding_emoji_image(emoji));
            let rules = self.onboarding_rules_channel(guild);
            let glyph = channel_glyph(known, channel, rules);
            let edit = task.clone();
            let group = SharedString::from(format!("onboarding-task-{index}"));
            list = list.child(
                div()
                    .mb(px(12.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id(group.clone())
                            .group(group.clone())
                            .relative()
                            .w_full()
                            .py(px(12.))
                            .px(px(16.))
                            .rounded(px(12.))
                            .bg(color("--background-base-lower"))
                            .when(error.is_some(), |this| {
                                this.border_2().border_color(color("--red-400")).mb(px(4.))
                            })
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(16.))
                                    .invisible()
                                    .group_hover(group, |style| style.visible())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_grab()
                                    .child(icon("ob_drag", 12., "--interactive-text-default")),
                            )
                            .child(if let Some(picture) = picture {
                                div()
                                    .size(px(48.))
                                    .flex_none()
                                    .rounded_full()
                                    .overflow_hidden()
                                    .child(img(picture).size(px(48.)))
                            } else {
                                div()
                                    .size(px(48.))
                                    .flex_none()
                                    .rounded_full()
                                    .bg(color("--background-base-lowest"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(if let Some(emoji) = emoji_picture {
                                        img(emoji).size(px(24.)).into_any_element()
                                    } else {
                                        icon(glyph, 24., "--interactive-text-default")
                                            .into_any_element()
                                    })
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .child(
                                        text(16., 20., 600., "--text-strong")
                                            .child(task.title.clone()),
                                    )
                                    .child(
                                        text(12., 16., 500., "--text-default")
                                            .child(format!("in #{}", channel.name)),
                                    ),
                            )
                            .child(
                                self.with_tip(
                                    format!("onboarding-task-edit-{index}"),
                                    Tip::text("Edit", Side::Top),
                                    icon_button(
                                        SharedString::from(format!("onboarding-task-edit-{index}")),
                                        "ob_pencil",
                                        Look::Primary,
                                        32.,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.onboarding_open_task(Some(&edit), window, cx);
                                        },
                                    )),
                                    cx,
                                ),
                            )
                            .on_drag(TaskDrag(index), |_, _, _, cx| cx.new(|_| DragGhost))
                            .on_drop(cx.listener(move |this, drag: &TaskDrag, _, cx| {
                                if let Some(state) = this.onboarding_mut()
                                    && let Some(tasks) = state.home.tasks.as_mut()
                                {
                                    *tasks = moved(tasks, drag.0, index);
                                }
                                cx.notify();
                            })),
                    )
                    .children(error.map(|message| {
                        text(12., 16., 500., "--text-feedback-critical").child(message)
                    })),
            );
        }
        list = list.child(
            div()
                .py(px(12.))
                .px(px(16.))
                .rounded(px(12.))
                .bg(color("--background-base-lower"))
                .flex()
                .items_center()
                .gap(px(16.))
                .child(
                    // `flex: 0 0 46px`, which an emoji inside would push out to 48.
                    div()
                        .w(px(46.))
                        .h(px(48.))
                        .flex_none()
                        .rounded_full()
                        .bg(color("--background-base-lowest"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            gpui::svg()
                                .path("icons/ob_book_check.svg")
                                .w(px(22.))
                                .h(px(20.))
                                .text_color(color("--interactive-text-default")),
                        ),
                )
                .child(text(16., 20., 600., "--text-strong").child("Read the rules")),
        );
        if count < MAX_TASKS {
            list = list.child(
                Self::onboarding_add_row("onboarding-add-task", "Add a task").on_click(
                    cx.listener(|this, _, window, cx| {
                        this.onboarding_open_task(None, window, cx);
                    }),
                ),
            );
        }
        list.into_any_element()
    }

    /// `addActionItem_nhgNJ0`.
    fn onboarding_add_row(id: &'static str, label: &'static str) -> Stateful<gpui::Div> {
        div()
            .id(id)
            .mt(px(12.))
            .h(px(56.))
            .p(px(16.))
            .rounded(px(8.))
            .border_2()
            .border_dashed()
            .border_color(color("--border-muted"))
            .cursor_pointer()
            .hover(|style| style.bg(color("--background-base-lower")))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(icon("ob_plus_circle", 16., "--text-link"))
            .child(text(16., 20., 400., "--text-link").child(label))
    }

    /// `pending` data URIs are decoded once by the image store under their own text.
    fn onboarding_data_image(&self, uri: &str) -> Option<std::sync::Arc<gpui::Image>> {
        self.images.get(uri).cloned()
    }

    /// `gb` and `gp`.
    fn onboarding_resources(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some((state, known)) = self.onboarding().zip(self.stores.guild(guild)) else {
            return div().into_any_element();
        };
        let resources = state.home.resources.clone().unwrap_or_default();
        let rules = self.onboarding_rules_channel(guild);
        let suggestions: Vec<&Channel> = selectable(known)
            .into_iter()
            .filter(|channel| {
                channel.kind == ChannelKind::Text
                    && Some(channel.id) != rules
                    && !state
                        .dismissed_suggestions
                        .contains(&channel.id.to_string())
                    && !resources
                        .iter()
                        .any(|resource| resource.channel_id == channel.id.to_string())
                    && everyone_can(known, channel, VIEW)
                    && !everyone_can(known, channel, SEND)
            })
            .take(5)
            .collect();
        let mut column = div().pr(px(8.)).flex().flex_col();
        if !suggestions.is_empty() && resources.len() < MAX_RESOURCES {
            let last = suggestions.len() - 1;
            let ids: Vec<String> = suggestions.iter().map(|c| c.id.to_string()).collect();
            let mut rows = div().mt(px(8.)).flex().flex_col();
            for (index, channel) in suggestions.iter().enumerate() {
                let id = channel.id.to_string();
                let title = channel.name.clone();
                rows = rows.child(
                    div()
                        .h(px(48.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon("ob_text", 20., "--icon-muted"))
                        .child(
                            text(14., 18., 400., "--text-default")
                                .flex_1()
                                .child(channel.name.clone()),
                        )
                        .child(
                            button(
                                SharedString::from(format!("onboarding-suggested-{id}")),
                                "Add",
                                Look::Primary,
                                false,
                                false,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(state) = this.onboarding_mut() {
                                        let list = state.home.resources.get_or_insert_default();
                                        list.retain(|known| known.channel_id != id);
                                        list.push(Resource {
                                            channel_id: id.clone(),
                                            title: title.clone(),
                                            ..Resource::default()
                                        });
                                    }
                                    cx.notify();
                                },
                            )),
                        ),
                );
                if index != last {
                    rows = rows.child(rule());
                }
            }
            column = column
                .child(heading_sm("Recommended"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            text(12., 16., 500., "--text-muted")
                                .child("We picked a few channels that are read-only"),
                        )
                        .child(
                            div()
                                .id("onboarding-suggestions-dismiss")
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(icon("ob_check", 16., "--text-brand"))
                                .child(text(12., 16., 600., "--text-brand").child("Dismiss All"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(state) = this.onboarding_mut() {
                                        state.dismissed_suggestions.extend(ids.iter().cloned());
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .child(rows)
                .child(div().my(px(16.)).child(rule()));
        }
        for (index, resource) in resources.iter().enumerate() {
            let channel = known
                .channels
                .iter()
                .find(|channel| channel.id.to_string() == resource.channel_id);
            let Some(channel) = channel else { continue };
            let error = (!resource_channel(known, channel))
                .then_some("Assigned channel must be readonly for @everyone");
            let picture =
                Self::resource_icon_url(resource).and_then(|url| self.images.get(&url).cloned());
            let edit = resource.clone();
            let group = SharedString::from(format!("onboarding-resource-{index}"));
            column = column.child(
                div()
                    .mb(px(12.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id(group.clone())
                            .group(group.clone())
                            .relative()
                            .w_full()
                            .py(px(12.))
                            .px(px(16.))
                            .rounded(px(12.))
                            .bg(color("--background-base-lower"))
                            .when(error.is_some(), |this| {
                                this.border_2().border_color(color("--red-400")).mb(px(4.))
                            })
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(16.))
                                    .invisible()
                                    .group_hover(group, |style| style.visible())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_grab()
                                    .child(icon("ob_drag", 12., "--interactive-text-default")),
                            )
                            .children(picture.map(|picture| {
                                div()
                                    .size(px(32.))
                                    .flex_none()
                                    .rounded(px(4.))
                                    .overflow_hidden()
                                    .child(img(picture).size(px(32.)))
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        text(16., 20., 600., "--text-strong")
                                            .child(resource.title.clone()),
                                    )
                                    .when(!resource.description.is_empty(), |this| {
                                        this.child(
                                            text(12., 16., 500., "--text-muted")
                                                .truncate()
                                                .child(resource.description.clone()),
                                        )
                                    }),
                            )
                            .child(
                                self.with_tip(
                                    format!("onboarding-resource-edit-{index}"),
                                    Tip::text("Edit", Side::Top),
                                    icon_button(
                                        SharedString::from(format!(
                                            "onboarding-resource-edit-{index}"
                                        )),
                                        "ob_pencil",
                                        Look::Primary,
                                        32.,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.onboarding_open_resource(Some(&edit), cx);
                                        },
                                    )),
                                    cx,
                                ),
                            )
                            .on_drag(ResourceDrag(index), |_, _, _, cx| cx.new(|_| DragGhost))
                            .on_drop(cx.listener(move |this, drag: &ResourceDrag, _, cx| {
                                if let Some(state) = this.onboarding_mut()
                                    && let Some(list) = state.home.resources.as_mut()
                                {
                                    *list = moved(list, drag.0, index);
                                }
                                cx.notify();
                            })),
                    )
                    .children(error.map(|message| {
                        text(12., 16., 500., "--text-feedback-critical").child(message)
                    })),
            );
        }
        if resources.len() < MAX_RESOURCES {
            column = column.child(
                Self::onboarding_add_row("onboarding-add-resource", "Add a resource")
                    .mt(px(0.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.onboarding_open_resource(None, cx);
                    })),
            );
        }
        column.into_any_element()
    }

    /// `gj`.
    fn onboarding_banner(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let known = self.stores.guild(guild);
        let unlocked = has_feature(known, "BANNER");
        let header = self
            .onboarding()
            .and_then(|state| state.home_header.clone());
        let picture = header.as_ref().and_then(|hash| {
            self.images
                .get(&format!(
                    "https://cdn.discordapp.com/home-headers/{guild}/{hash}.png?size=1024"
                ))
                .cloned()
        });
        let description = text(14., 18., 400., "--text-default")
            .child("The recommended minimum size is 1920x480 and recommended aspect ratio is 4:1.");
        let boost = |this: &mut Self, cx: &mut Context<Self>| {
            this.open_perks(super::server_boost::Perks::Banner, cx);
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(32.))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(heading_lg("Server Guide Banner"))
                    .child(
                        div()
                            .id("onboarding-banner-level")
                            .mt(px(3.))
                            .h(px(18.))
                            .px(px(4.))
                            .rounded(px(12.))
                            .bg(color("--background-mod-normal"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(3.))
                            .child(icon("ob_boost_level", 14., "--interactive-text-default"))
                            .child(
                                text(14., 18., 500., "--interactive-text-default").child("LVL 2"),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| boost(this, cx))),
                    ),
            )
            .child(
                div()
                    .mt(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(description)
                    .child(if unlocked {
                        button(
                            "onboarding-banner-upload",
                            "Upload Background",
                            Look::Primary,
                            true,
                            false,
                        )
                        .w_auto()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.onboarding_pick_picture(
                                |this, uri, cx| this.onboarding_home_header(Some(uri), cx),
                                cx,
                            );
                        }))
                    } else {
                        div()
                            .id("onboarding-banner-unlock")
                            .mt(px(8.))
                            .flex_none()
                            .h(px(40.))
                            .w(px(201.7))
                            .rounded(px(8.))
                            .bg(gpui::white())
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(4.))
                            .child(
                                gpui::svg()
                                    .path("icons/boost_gem.svg")
                                    .size(px(20.))
                                    .text_color(gpui::black()),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(gpui::black())
                                    .child("Unlock with Boosting"),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| boost(this, cx)))
                    }),
            )
            .child(
                div()
                    .id("onboarding-banner-image")
                    .mt(px(16.))
                    .relative()
                    .w(px(364.))
                    .h(px(180.))
                    .rounded(px(12.))
                    .overflow_hidden()
                    .bg(color("--background-mod-strong"))
                    .cursor_pointer()
                    .children(picture.map(|picture| img(picture).w(px(364.)).h(px(180.))))
                    .child(
                        div()
                            .absolute()
                            .right(px(16.))
                            .top(px(16.))
                            .size(px(28.))
                            .rounded_full()
                            .bg(gpui::white())
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("boost_add_image", 18., "--background-mod-strong")),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if unlocked {
                            this.onboarding_pick_picture(
                                |this, uri, cx| this.onboarding_home_header(Some(uri), cx),
                                cx,
                            );
                        } else {
                            boost(this, cx);
                        }
                    })),
            )
            .into_any_element()
    }

    /// `gZ`: `completed` is the page once Onboarding was ever on.
    fn onboarding_review(&self, guild: Id, completed: bool, cx: &mut Context<Self>) -> AnyElement {
        let Some((state, known)) = self.onboarding().zip(self.stores.guild(guild)) else {
            return div().into_any_element();
        };
        let ids = state.saved.default_channels.clone();
        let (_, all) = default_reach(known, &ids);
        let channel_count = all.len();
        let advanced = state.saved.mode == 1;
        let from_questions = required_reach(known, &ids, &state.saved.prompts, false)
            .len()
            .saturating_sub(channel_count);
        let plural = |count: usize, one: &str, many: &str| {
            if count == 1 {
                format!("{count} {one}")
            } else {
                format!("{count} {many}")
            }
        };
        let channels_started = channel_count > 0 || advanced;
        let channels_text = if advanced {
            format!(
                "You have {} and at least {} assigned by questions.",
                plural(channel_count, "Default Channel", "Default Channels"),
                plural(from_questions, "channel", "channels"),
            )
        } else {
            format!(
                "You have {}",
                plural(channel_count, "Default Channel", "Default Channels")
            )
        };
        let (covered, missing) = coverage(known, &ids, &state.saved.prompts);
        let total = covered.len() + missing.len();
        let percent = if total == 0 {
            f32::NAN
        } else {
            covered.len() as f32 / total as f32 * 100.
        };
        let questions_started = !state.saved.prompts.is_empty();
        let questions_status = if !questions_started {
            Status::None
        } else if percent >= 85. {
            if completed {
                Status::None
            } else {
                Status::Good
            }
        } else if percent < 85. {
            Status::Warning
        } else {
            Status::None
        };
        let count_text = format!("{} of {total}", covered.len());
        let question_rest =
            " public channels are assignable through Questions and Default Channels.";
        let question_text = text(12., 16., 400., "--text-default")
            .child(
                StyledText::new(format!("{count_text}{question_rest}")).with_highlights(vec![(
                    0..count_text.len(),
                    HighlightStyle {
                        color: Some(if percent > 85. {
                            super::hsla("--text-strong")
                        } else {
                            super::hsla("--status-warning")
                        }),
                        ..HighlightStyle::default()
                    },
                )]),
            )
            .into_any_element();
        let viewable = self.onboarding_viewable(guild);
        let settings = &state.home;
        let guide_valid = settings.is_valid(&viewable);
        let guide_started = !settings.is_empty();
        let guide_status = if !guide_started {
            Status::None
        } else if !guide_valid {
            Status::Required
        } else if completed {
            Status::None
        } else {
            Status::Good
        };
        let guide_text = if !guide_valid {
            GUIDE_INVALID
        } else if !guide_started {
            "Set up your Server Guide to help new members get started."
        } else {
            "Your Welcome Message, Banner, To Do tasks, and Resources are all set up"
        };
        let guide_tint = if guide_started && !guide_valid {
            "--text-feedback-critical"
        } else {
            "--text-default"
        };
        let guide_switch = (guide_started && state.saved.enabled).then(|| {
            let on = settings.enabled;
            div()
                .id("onboarding-guide-switch")
                .mr(px(16.))
                .when(guide_valid, |this| this.cursor_pointer())
                .child(if guide_valid {
                    super::create_channel::switch(on)
                } else {
                    super::create_channel::switch_dimmed(on, "--background-base-lower")
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if guide_valid {
                        this.onboarding_set_guide_enabled(!on, cx);
                    }
                }))
                .into_any_element()
        });
        let divider = || div().mx(px(16.)).h(px(1.)).bg(color("--border-subtle"));
        let rows = div()
            .flex()
            .flex_col()
            .child(
                self.onboarding_review_row(
                    "channels",
                    if advanced {
                        "Default Channels & Pre-join Questions"
                    } else {
                        "Default Channels"
                    },
                    text(12., 16., 400., "--text-default")
                        .child(channels_text)
                        .into_any_element(),
                    "ob_text",
                    channels_started,
                    if channels_started && !completed {
                        Status::Good
                    } else {
                        Status::None
                    },
                    None,
                    Step::Channels,
                    cx,
                ),
            )
            // `gU.A` twice: `gq` draws nothing between its two dividers.
            .child(divider())
            .child(divider())
            .child(self.onboarding_review_row(
                "questions",
                if advanced {
                    "Post-join Questions"
                } else {
                    "Questions"
                },
                question_text,
                "ob_friends",
                questions_started,
                questions_status,
                None,
                Step::Questions,
                cx,
            ))
            .child(divider())
            .child(
                self.onboarding_review_row(
                    "guide",
                    "Server Guide",
                    text(12., 16., 400., guide_tint)
                        .child(guide_text)
                        .into_any_element(),
                    "ob_signpost",
                    guide_started,
                    guide_status,
                    guide_switch,
                    Step::Home,
                    cx,
                ),
            );
        if completed {
            let enabled = state.saved.enabled;
            let label = if enabled {
                "Onboarding is Enabled"
            } else {
                "Onboarding is Disabled"
            };
            return div()
                .flex()
                .flex_col()
                .child(heading_lg("Onboarding"))
                .child(
                    div().mt(px(8.)).w(px(670.)).child(
                        text(16., 20., 400., "--text-default").child(
                            "Give your members a simple starting experience with custom channels, roles, and first steps.",
                        ),
                    ),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            link("See examples")
                                .id("onboarding-examples-edit")
                                .on_click(cx.listener(|_, _, _, cx| cx.open_url(EXAMPLES))),
                        )
                        .child(link_dot())
                        // `r`: the full server preview does not exist yet.
                        .child(link("Preview").id("onboarding-preview-edit"))
                        .child(link_dot())
                        .child(self.onboarding_mode_link(cx)),
                )
                .child(
                    div()
                        .mt(px(16.))
                        .p(px(16.))
                        .rounded(px(8.))
                        .bg(color("--background-base-lower"))
                        .flex()
                        .flex_col()
                        .child(heading_sm("Recent Updates"))
                        .child(div().mt(px(8.)).child(body_sm_medium(
                            "\u{2022} You can now upload custom images for New Member To Dos and Resource Pages.",
                        )))
                        .child(body_sm_medium(
                            "\u{2022} Added a custom description option for Resource pages",
                        )),
                )
                .child(
                    div()
                        .mt(px(24.))
                        .rounded(px(8.))
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id("onboarding-toggle")
                                .py(px(12.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--background-base-lowest"))
                                .cursor_pointer()
                                .flex()
                                .items_start()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .child(text(16., 20., 500., "--text-strong").child(label))
                                        .child(
                                            text(14., 18., 400., "--text-subtle")
                                                .child("Changes will not take effect until you save."),
                                        ),
                                )
                                .child(super::create_channel::switch(enabled))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.onboarding_toggle(false, cx);
                                })),
                        )
                        .child(rows),
                )
                .into_any_element();
        }
        let very_high = state.verification >= VERY_HIGH;
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .rounded(px(8.))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .py(px(12.))
                            .px(px(16.))
                            .rounded_t(px(8.))
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    // `gY` tests `length >= 0` here, so this build is always ready.
                                    .child(heading_sm("You're ready to enable"))
                                    .child(text(12., 16., 400., "--text-default").child(
                                        "Push the big blue button to turn on your new member experience!",
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    // `r`: the full server preview does not exist yet.
                                    .child(
                                        button(
                                            "onboarding-review-preview",
                                            "Preview",
                                            Look::Secondary,
                                            false,
                                            false,
                                        )
                                        .w(px(71.7)),
                                    )
                                    .child(
                                        button(
                                            "onboarding-enable",
                                            "Enable Onboarding",
                                            Look::Primary,
                                            false,
                                            false,
                                        )
                                        .w(px(138.3))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.onboarding_toggle(guide_valid, cx);
                                        })),
                                    ),
                            ),
                    )
                    .child(rows),
            )
            .when(very_high, |this| {
                this.child(
                    div()
                        .mt(px(16.))
                        .flex()
                        .gap(px(8.))
                        .child(icon("ob_warning", 12., "--text-muted"))
                        .child(text(14., 18., 500., "--text-muted").child(
                            "Once enabled, getting a role will not bypass a member through verification and Rules. To manually verify members, you can right click a user and select Verify Member, or use the Members tab.",
                        )),
                )
            })
            .into_any_element()
    }

    /// `gz`.
    #[allow(clippy::too_many_arguments)]
    fn onboarding_review_row(
        &self,
        id: &'static str,
        title: &'static str,
        description: AnyElement,
        glyph: &'static str,
        started: bool,
        status: Status,
        extra: Option<AnyElement>,
        target: Step,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let badge = match status {
            Status::None => None,
            Status::Good => Some(("ob_check", "--status-positive", "Good")),
            Status::Warning => Some(("ob_warning", "--text-feedback-warning", "Warning")),
            Status::Required => Some(("ob_warning", "--text-feedback-critical", "Required")),
        };
        let open = cx.listener(
            move |this: &mut Self,
                  _: &gpui::ClickEvent,
                  window: &mut Window,
                  cx: &mut Context<Self>| {
                this.onboarding_go(target, Some(window), cx);
            },
        );
        div()
            .p(px(16.))
            .bg(color("--background-base-lower"))
            // `content_SUjSf4` has round corners; gpui clips children square, so the last row carries them.
            .when(id == "guide", |this| this.rounded_b(px(8.)))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .size(px(40.))
                            .flex_none()
                            .rounded(px(48.))
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(glyph, 24., "--interactive-text-default")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(heading_sm(title))
                            .child(description),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .children(badge.map(|(glyph, tint, label)| {
                        div()
                            .mr(px(16.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon(
                                glyph,
                                16.,
                                if status == Status::Warning {
                                    "--status-warning"
                                } else {
                                    tint
                                },
                            ))
                            .child(text(14., 18.67, 500., tint).child(label))
                    }))
                    .children(extra)
                    .child(if started {
                        button(
                            SharedString::from(format!("onboarding-edit-{id}")),
                            "Edit",
                            Look::Secondary,
                            false,
                            false,
                        )
                        .w(px(60.))
                        .on_click(open)
                    } else {
                        button(
                            SharedString::from(format!("onboarding-setup-{id}")),
                            "Set up",
                            Look::Secondary,
                            false,
                            false,
                        )
                        .w(px(82.8))
                        .child(icon(
                            "ob_arrow_right",
                            16.,
                            "--control-secondary-text-default",
                        ))
                        .on_click(open)
                    }),
            )
            .into_any_element()
    }

    /// Every overlay of the page, drawn from the Root.
    pub(super) fn onboarding_layer(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut layers = Vec::new();
        let Some(open) = self.server_settings.as_ref() else {
            return layers;
        };
        if open.page != Page::Onboarding {
            return layers;
        }
        let Some(state) = open.onboarding.as_ref() else {
            return layers;
        };
        match state.dialog.as_ref() {
            Some(Dialog::Answer(dialog)) => layers.push(self.onboarding_answer_modal(dialog, cx)),
            Some(Dialog::Task(dialog)) => layers.push(self.onboarding_task_modal(dialog, cx)),
            Some(Dialog::Resource(dialog)) => {
                layers.push(self.onboarding_resource_modal(dialog, cx))
            }
            Some(Dialog::Welcome(dialog)) => layers.push(self.onboarding_welcome_modal(dialog, cx)),
            Some(Dialog::Unlock(dialog)) => layers.push(self.onboarding_unlock_modal(dialog, cx)),
            None => {}
        }
        layers.extend(self.onboarding_dropdown(cx));
        layers.extend(self.onboarding_confirm_modal(state, cx));
        layers
    }

    /// `b.A` in `mA`, module 157559's alert.
    fn onboarding_confirm_modal(
        &self,
        state: &OnboardingState,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let close = |this: &mut Root, _: &mut Window, _: &mut Context<Root>| {
            if let Some(state) = this.onboarding_mut() {
                state.confirm = None;
            }
        };
        let spec = match state.confirm.as_ref()? {
            Confirm::Mode { advanced } => {
                let advanced = *advanced;
                let (title, body, label) = if advanced {
                    (
                        "Advanced Server Setup",
                        "Have an advanced server setup? Enable this mode to meet requirements via Default Channels and Questions combined.",
                        "Enable",
                    )
                } else {
                    (
                        "Disable Advanced Server Setup",
                        "Return to the regular requirements where at least one Default Channel must be chattable.",
                        "Disable",
                    )
                };
                modal::Spec {
                    id: "onboarding-mode-dialog",
                    title: title.into(),
                    subtitle: Some(div().child(body).into_any_element()),
                    body: None,
                    buttons: vec![
                        modal::button("onboarding-mode-cancel", "Cancel", Tone::Secondary, close),
                        modal::button(
                            "onboarding-mode-confirm",
                            label,
                            Tone::Primary,
                            move |this, _, cx| {
                                if let Some(state) = this.onboarding_mut() {
                                    state.confirm = None;
                                }
                                this.onboarding_set_mode(advanced, cx);
                            },
                        ),
                    ],
                    close: Box::new(close),
                }
            }
            Confirm::Alert { title, body } => modal::Spec {
                id: "onboarding-alert",
                title: title.clone().into(),
                subtitle: Some(div().child(body.clone()).into_any_element()),
                body: None,
                buttons: vec![modal::button(
                    "onboarding-alert-ok",
                    "Okay",
                    Tone::Primary,
                    close,
                )],
                close: Box::new(close),
            },
            Confirm::Dropdown { prompt, index } => {
                let (prompt, index) = (prompt.clone(), *index);
                modal::Spec {
                    id: "onboarding-dropdown-note",
                    title: "Changing answers to a dropdown".into(),
                    subtitle: Some(
                        div()
                            .child("When adding 13 or more answers to a question, we show them to members as dropdown options without descriptions.")
                            .into_any_element(),
                    ),
                    body: None,
                    buttons: vec![
                        modal::button("onboarding-dropdown-cancel", "Cancel", Tone::Secondary, close),
                        modal::button(
                            "onboarding-dropdown-ok",
                            "Okay",
                            Tone::Primary,
                            move |this, window, cx| {
                                if let Some(state) = this.onboarding_mut() {
                                    state.confirm = None;
                                }
                                this.onboarding_open_answer(&prompt, None, index, window, cx);
                            },
                        ),
                    ],
                    close: Box::new(close),
                }
            }
        };
        Some(modal::render_small(spec, cx))
    }

    /// `o.a` (module 189213): the header, the body that scrolls between lines, and an action bar of a critical text button
    /// on the left with two buttons on the right, or two across it.
    #[allow(clippy::too_many_arguments)]
    fn onboarding_dialog(
        &self,
        id: &'static str,
        title: SharedString,
        subtitle: Option<SharedString>,
        scrolls: bool,
        body: AnyElement,
        footer: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let close = |this: &mut Self, cx: &mut Context<Self>| {
            if let Some(state) = this.onboarding_mut() {
                state.dialog = None;
                state.dropdown = None;
            }
            cx.notify();
        };
        let panel = div()
            .id(id)
            .occlude()
            .w(px(480.))
            .max_h(px(608.))
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
            .child(
                div()
                    .flex_none()
                    .pt(px(16.))
                    .px(px(24.))
                    .pb(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .h(px(24.))
                            .flex()
                            .child(div().flex_1().child(heading_lg(title)))
                            .child(
                                div()
                                    .id(SharedString::from(format!("{id}-close")))
                                    .mt(px(-16.))
                                    .mr(px(-16.))
                                    .size(px(40.))
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(
                                        "modal_close",
                                        20.,
                                        "--control-icon-only-icon-default",
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| close(this, cx))),
                            ),
                    )
                    .children(
                        subtitle
                            .map(|subtitle| text(16., 20., 400., "--text-subtle").child(subtitle)),
                    ),
            )
            // `bodySpacerTopBorder` and `bodyScrollingSpacerBottom`: lines at the edges of a body that overflows.
            .child(
                div()
                    .flex_none()
                    .h(px(if scrolls { 17. } else { 16. }))
                    .when(scrolls, |this| {
                        this.border_b_1().border_color(color("--border-subtle"))
                    }),
            )
            .child(
                div()
                    .id(SharedString::from(format!("{id}-body")))
                    .flex_1()
                    .min_h(px(0.))
                    .pl(px(24.))
                    .pr(px(24.))
                    .overflow_y_scroll()
                    .child(body),
            )
            .child(
                div()
                    .flex_none()
                    .h(px(if scrolls { 9. } else { 8. }))
                    .when(scrolls, |this| {
                        this.border_t_1().border_color(color("--border-subtle"))
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .pt(px(16.))
                    .px(px(24.))
                    .pb(px(8.))
                    .child(footer),
            );
        deferred(
            div()
                .id(SharedString::from(format!("{id}-layer")))
                .occlude()
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b8))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(move |this, _, _, cx| close(this, cx)))
                .child(panel),
        )
        .with_priority(1)
        .into_any_element()
    }

    /// An element whose place a dropdown opens from.
    fn onboarding_anchored(&self, key: &'static str, target: impl IntoElement) -> gpui::Div {
        let anchors = self.tip_anchors.clone();
        div().relative().child(target).child(
            gpui::canvas(
                move |bounds, window, _| {
                    let mut anchors = anchors.borrow_mut();
                    if anchors.get(key) != Some(&bounds) {
                        anchors.insert(key.into(), bounds);
                        window.request_animation_frame();
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    }

    /// `h.$` with a red star.
    fn onboarding_form_title(label: &'static str, required: bool, medium: bool) -> gpui::Div {
        let weight = if medium { 500. } else { 600. };
        div()
            .flex()
            .gap(px(4.))
            .child(text(16., 20., weight, "--text-strong").child(label))
            .when(required, |this| {
                this.child(text(16., 20., weight, "--text-feedback-critical").child("*"))
            })
    }

    fn onboarding_footer_buttons(
        &self,
        id: &'static str,
        remove: Option<AnyElement>,
        save_enabled: bool,
        save: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let has_remove = remove.is_some();
        let width = if has_remove { px(100.) } else { px(211.) };
        div()
            .flex()
            .items_center()
            .justify_between()
            .when_some(remove, |this, remove| this.child(remove))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        button(
                            SharedString::from(format!("{id}-cancel")),
                            "Cancel",
                            Look::Secondary,
                            true,
                            false,
                        )
                        .w(width)
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(state) = this.onboarding_mut() {
                                state.dialog = None;
                                state.dropdown = None;
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        button(
                            SharedString::from(format!("{id}-save")),
                            "Save",
                            Look::Primary,
                            true,
                            !save_enabled,
                        )
                        .w(width)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if save_enabled {
                                save(this, cx);
                            }
                        })),
                    ),
            )
            .into_any_element()
    }

    fn onboarding_remove_button(
        id: &'static str,
        run: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .cursor_pointer()
            .child(text(16., 20., 500., "--text-feedback-critical").child("Remove"))
            .on_click(cx.listener(move |this, _, _, cx| run(this, cx)))
            .into_any_element()
    }

    /// Module 165394.
    fn onboarding_answer_modal(&self, dialog: &AnswerDialog, cx: &mut Context<Self>) -> AnyElement {
        let guild = self.onboarding().map_or(0, |state| state.guild);
        let known = self.stores.guild(guild);
        // `divider_ZaYWLb` inside the stack: a 1px line in a box 2 tall.
        let separator = || {
            div()
                .my(px(16.))
                .h(px(2.))
                .border_t_1()
                .border_color(color("--border-subtle"))
        };
        let tokens = |ids: &[String], roles: bool, cx: &mut Context<Self>| {
            ids.iter()
                .map(|id| {
                    let label = known.and_then(|known| {
                        if roles {
                            id.parse::<Id>()
                                .ok()
                                .and_then(|id| known.roles.get(&id).cloned())
                        } else {
                            known
                                .channels
                                .iter()
                                .find(|channel| channel.id.to_string() == *id)
                                .map(|channel| channel.name.clone())
                        }
                    });
                    let glyph = known.and_then(|known| {
                        known
                            .channels
                            .iter()
                            .find(|channel| channel.id.to_string() == *id)
                            .map(|channel| channel_glyph(known, channel, None))
                    });
                    let remove = id.clone();
                    div()
                        .id(SharedString::from(format!("onboarding-token-{id}")))
                        .h(px(26.))
                        .pr(px(8.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-mod-normal"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .pl(px(12.))
                                .pr(px(8.))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .children(glyph.map(|glyph| icon(glyph, 16., "--text-muted")))
                                .child(body_sm_medium(label.unwrap_or_default())),
                        )
                        .child(icon("ob_x", 12., "--text-default"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(Dialog::Answer(dialog)) = this
                                .onboarding_mut()
                                .and_then(|state| state.dialog.as_mut())
                            {
                                if roles {
                                    dialog.roles.retain(|known| *known != remove);
                                } else {
                                    dialog.channels.retain(|known| *known != remove);
                                }
                            }
                            cx.notify();
                        }))
                        .into_any_element()
                })
                .collect::<Vec<_>>()
        };
        let search = |key: &'static str,
                      dropdown: Dropdown,
                      field: &Field,
                      ids: &[String],
                      roles: bool,
                      helper: &'static str,
                      cx: &mut Context<Self>| {
            let list = tokens(ids, roles, cx);
            let handle = field.editor.clone();
            div()
                .flex()
                .flex_col()
                .child(
                    self.onboarding_anchored(
                        key,
                        div()
                            .id(key)
                            .w(px(430.))
                            .min_h(px(36.))
                            .p(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .cursor_text()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(4.))
                            .children(list)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(100.))
                                    .px(px(6.))
                                    .child(field.editor.clone()),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let focus = handle.read(cx).focus_handle(cx);
                                window.focus(&focus, cx);
                                if let Some(state) = this.onboarding_mut() {
                                    state.dropdown = Some(dropdown);
                                }
                                cx.notify();
                            })),
                    ),
                )
                .child(
                    div()
                        .py(px(4.))
                        .child(text(12., 16., 400., "--text-muted").child(helper)),
                )
        };
        let picture = dialog
            .emoji
            .as_ref()
            .and_then(|emoji| self.onboarding_emoji_image(emoji));
        let has_emoji = dialog.emoji.is_some();
        let body = div()
            .pt(px(12.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(Self::onboarding_form_title("1. Add an answer", true, true))
                    .child(self.onboarding_input(
                        &dialog.title.editor,
                        dialog.title_error.is_some(),
                        40.,
                        cx,
                    ))
                    .children(dialog.title_error.map(|message| {
                        text(12., 16., 400., "--text-feedback-critical").child(message)
                    }))
                    .child(self.onboarding_input(&dialog.description.editor, false, 40., cx)),
            )
            .child(separator())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(Self::onboarding_form_title(
                        "2. Assign channels and/or roles",
                        false,
                        true,
                    ))
                    .child(search(
                        "onboarding-answer-channels",
                        Dropdown::Channels,
                        &dialog.channel_query,
                        &dialog.channels,
                        false,
                        "@everyone must have View permission for channels used in answers",
                        cx,
                    ))
                    .child(search(
                        "onboarding-answer-roles",
                        Dropdown::Roles,
                        &dialog.role_query,
                        &dialog.roles,
                        true,
                        "Members will receive all channels granted by these roles",
                        cx,
                    ))
                    .children(dialog.roles_error.map(|message| {
                        text(12., 16., 400., "--text-feedback-critical").child(message)
                    })),
            )
            .child(separator())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(Self::onboarding_form_title("3. Pick an emoji", false, true))
                    .child(
                        div().mt(px(4.)).child(
                            text(14., 18., 400., "--text-subtle")
                                .child("Optional, but why not add some fun?"),
                        ),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                self.onboarding_anchored(
                                    "onboarding-answer-emoji",
                                    div()
                                        .id("onboarding-answer-emoji")
                                        .size(px(42.))
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(color("--background-base-lowest"))
                                        .bg(color("--background-mod-normal"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(if let Some(picture) = picture {
                                            img(picture).size(px(24.)).into_any_element()
                                        } else {
                                            icon(
                                                "sound_no_emoji",
                                                18.,
                                                "--interactive-text-default",
                                            )
                                            .into_any_element()
                                        })
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            let anchor = this
                                                .tip_anchors
                                                .borrow()
                                                .get("onboarding-answer-emoji")
                                                .copied();
                                            if let Some(bounds) = anchor {
                                                this.open_picker(
                                                    super::picker::Target::OnboardingEmoji,
                                                    gpui::point(bounds.left(), bounds.bottom()),
                                                    window,
                                                    cx,
                                                );
                                            }
                                        })),
                                ),
                            )
                            .when(has_emoji, |this| {
                                this.child(
                                    div()
                                        .id("onboarding-answer-emoji-clear")
                                        .cursor_pointer()
                                        .child(icon(
                                            "circle_x_small",
                                            16.,
                                            "--interactive-text-default",
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(Dialog::Answer(dialog)) = this
                                                .onboarding_mut()
                                                .and_then(|state| state.dialog.as_mut())
                                            {
                                                dialog.emoji = None;
                                            }
                                            cx.notify();
                                        })),
                                )
                            }),
                    ),
            )
            .into_any_element();
        let footer = self.onboarding_footer_buttons(
            "onboarding-answer",
            Some(Self::onboarding_remove_button(
                "onboarding-answer-remove",
                |this, cx| this.onboarding_remove_answer(cx),
                cx,
            )),
            true,
            |this, cx| this.onboarding_save_answer(cx),
            cx,
        );
        self.onboarding_dialog(
            "onboarding-answer-dialog",
            format!("Question {}", dialog.index + 1).into(),
            Some(dialog.prompt_title.clone().into()),
            true,
            body,
            footer,
            cx,
        )
    }

    /// The combo box `h.Z`, closed.
    fn onboarding_select(
        &self,
        key: &'static str,
        value: Option<(Option<&'static str>, String)>,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.onboarding_anchored(
            key,
            div()
                .id(key)
                .w(px(430.))
                .h(px(40.))
                .pl(px(12.))
                .pr(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(color(if open {
                    "--input-border-active"
                } else {
                    "--input-border-default"
                }))
                .rule(crate::ui::motion::Rule::TextField)
                .bg(color("--input-background-default"))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_between()
                .child(match value {
                    Some((glyph, label)) => div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .children(glyph.map(|glyph| icon(glyph, 16., "--text-muted")))
                        .child(text(16., 20., 400., "--text-default").child(label))
                        .into_any_element(),
                    None => text(16., 20., 400., "--input-placeholder-text-default")
                        .pt(px(1.))
                        .child("Select...")
                        .into_any_element(),
                })
                .child(if open {
                    gpui::svg()
                        .path("icons/select_chevron.svg")
                        .size(px(20.))
                        .text_color(color("--text-strong"))
                        .with_transformation(gpui::Transformation::rotate(gpui::percentage(0.5)))
                        .into_any_element()
                } else {
                    icon("select_chevron", 20., "--text-strong").into_any_element()
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(state) = this.onboarding_mut() {
                        state.dropdown = match state.dropdown {
                            Some(Dropdown::Single) => None,
                            _ => Some(Dropdown::Single),
                        };
                    }
                    cx.notify();
                })),
        )
        .into_any_element()
    }

    /// Module 930345.
    fn onboarding_welcome_modal(
        &self,
        dialog: &WelcomeDialog,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let message = dialog.message.text(cx);
        let error = welcome_error(dialog.author.is_some(), &message);
        let open = self
            .onboarding()
            .is_some_and(|state| state.dropdown == Some(Dropdown::Single));
        let author = dialog.author.as_ref().and_then(|id| {
            let id: Id = id.parse().ok()?;
            let me = self.stores.current_user.as_ref();
            me.filter(|me| me.id == id)
                .or_else(|| self.stores.users.get(&id))
                .map(|user| user.display_name().to_owned())
        });
        let body = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(Self::onboarding_form_title("Message Author", true, false))
                    .child(body_sm(
                        "Members with permissions to manage the guild and roles.",
                    ))
                    .child(self.onboarding_select(
                        "onboarding-welcome-author",
                        author.map(|name| (None, name)),
                        open,
                        cx,
                    )),
            )
            .child(div().my(px(24.)).child(rule()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(Self::onboarding_form_title("Welcome Message", false, false))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(
                                StyledText::new(
                                    "TIP: Use [@username] to mention the member by name.",
                                )
                                .with_highlights(vec![(
                                    0..4,
                                    HighlightStyle {
                                        font_weight: Some(FontWeight(600.)),
                                        ..HighlightStyle::default()
                                    },
                                )]),
                            ),
                    )
                    .child(self.onboarding_text_area(
                        &dialog.message.editor,
                        WELCOME_PLACEHOLDER,
                        89.,
                        cx,
                    ))
                    .children(error.clone().map(|message| {
                        text(12., 16., 400., "--text-feedback-critical").child(message)
                    })),
            )
            .into_any_element();
        let footer = self.onboarding_footer_buttons(
            "onboarding-welcome",
            Some(Self::onboarding_remove_button(
                "onboarding-welcome-remove",
                |this, cx| this.onboarding_save_welcome(true, cx),
                cx,
            )),
            !message.is_empty() && error.is_none(),
            |this, cx| this.onboarding_save_welcome(false, cx),
            cx,
        );
        self.onboarding_dialog(
            "onboarding-welcome-dialog",
            "Set up welcome sign".into(),
            None,
            false,
            body,
            footer,
            cx,
        )
    }

    /// `x.f`. The editor's own placeholder keeps to one line, so an empty area draws its own, which wraps.
    fn onboarding_text_area(
        &self,
        editor: &Entity<Editor>,
        placeholder: &'static str,
        height: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let focused = self
            .onboarding()
            .is_some_and(|state| state.focused == Some(editor.entity_id()));
        let empty = editor.read(cx).text().is_empty();
        let target = editor.clone();
        div()
            .id(SharedString::from(format!(
                "onboarding-area-{}",
                editor.entity_id()
            )))
            .cursor_text()
            .on_click(cx.listener(move |_, _, window, cx| {
                let handle = target.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }))
            .relative()
            .h(px(height))
            .w_full()
            .py(px(12.))
            .px(px(11.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(if focused {
                "--input-border-active"
            } else {
                "--input-border-default"
            }))
            .rule(crate::ui::motion::Rule::TextField)
            .bg(color("--input-background-default"))
            .overflow_hidden()
            .when(empty, |this| {
                this.child(
                    div()
                        .absolute()
                        .left(px(10.))
                        .top(px(12.))
                        .w(px(400.))
                        .text_size(px(16.))
                        .line_height(px(21.))
                        .text_color(color("--input-placeholder-text-default"))
                        .child(placeholder),
                )
            })
            .child(editor.clone())
    }

    /// Module 251632.
    fn onboarding_task_modal(&self, dialog: &TaskDialog, cx: &mut Context<Self>) -> AnyElement {
        let guild = self.onboarding().map_or(0, |state| state.guild);
        let known = self.stores.guild(guild);
        let found = dialog.channel.as_ref().and_then(|id| {
            known.and_then(|known| known.channels.iter().find(|c| c.id.to_string() == *id))
        });
        let rules = self.onboarding_rules_channel(guild);
        let value = found.zip(known).map(|(channel, known)| {
            (
                Some(channel_glyph(known, channel, rules)),
                channel.name.clone(),
            )
        });
        let open = self
            .onboarding()
            .is_some_and(|state| state.dropdown == Some(Dropdown::Single));
        let title = dialog.title.text(cx);
        let title_error = if title.is_empty() {
            None
        } else {
            task_title_error(&title)
        };
        let chat_label = match found.map(|channel| channel.kind) {
            Some(ChannelKind::Other(16)) => "Member posts or responds in the media channel",
            Some(ChannelKind::Other(15)) => "Member posts or responds in the forum",
            _ => "Member sends a message in the channel",
        };
        let radio = |id: &'static str, label: &'static str, value: u64, cx: &mut Context<Self>| {
            let allowed = found
                .zip(known)
                .is_none_or(|(channel, known)| can_complete(known, channel, value));
            div()
                .id(id)
                .h(px(20.))
                .when(allowed, |this| this.cursor_pointer())
                .when(!allowed, |this| this.opacity(0.5))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(super::create_channel::radio(dialog.action == Some(value)))
                .child(text(16., 20., 400., "--text-default").child(label))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if allowed {
                        if let Some(Dialog::Task(dialog)) = this
                            .onboarding_mut()
                            .and_then(|state| state.dialog.as_mut())
                        {
                            dialog.action = Some(value);
                        }
                        this.onboarding_task_recheck();
                        cx.notify();
                    }
                }))
        };
        let picture = dialog
            .icon
            .as_ref()
            .and_then(|uri| self.onboarding_data_image(uri))
            .or_else(|| {
                dialog
                    .icon_key
                    .as_ref()
                    .and_then(|url| self.images.get(url).cloned())
            });
        let emoji_picture = dialog
            .emoji
            .as_ref()
            .and_then(|emoji| self.onboarding_emoji_image(emoji));
        let has_image = picture.is_some() || emoji_picture.is_some();
        let thumb =
            self.onboarding_anchored(
                "onboarding-task-thumb",
                div()
                    .id("onboarding-task-thumb")
                    .cursor_pointer()
                    .child(if let Some(shown) = picture.or(emoji_picture) {
                        div()
                            .relative()
                            .size(px(56.))
                            .child(img(shown).size(px(56.)))
                            .child(div().absolute().right(px(-4.)).bottom(px(-4.)).child(
                                icon_button(
                                    "onboarding-task-thumb-edit",
                                    "ob_pencil",
                                    Look::Secondary,
                                    32.,
                                ),
                            ))
                            .into_any_element()
                    } else {
                        icon_button(
                            "onboarding-task-thumb-add",
                            "ob_image_plus",
                            Look::Secondary,
                            40.,
                        )
                        .into_any_element()
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this.onboarding_mut() {
                            state.dropdown = match state.dropdown {
                                Some(Dropdown::ThumbMenu) => None,
                                _ => Some(Dropdown::ThumbMenu),
                            };
                        }
                        cx.notify();
                    })),
            );
        let separator = || div().my(px(24.)).child(rule());
        let body = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(Self::onboarding_form_title(
                        "What should the new member do?",
                        true,
                        false,
                    ))
                    .child(self.onboarding_input(
                        &dialog.title.editor,
                        title_error.is_some(),
                        40.,
                        cx,
                    ))
                    .children(title_error.map(|message| {
                        text(12., 16., 400., "--text-feedback-critical").child(message)
                    })),
            )
            .child(separator())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(Self::onboarding_form_title(
                        "Where should they do it?",
                        true,
                        false,
                    ))
                    .child(self.onboarding_select("onboarding-task-channel", value, open, cx))
                    .child(
                        text(12., 16., 500., "--text-muted")
                            .child("Channels must be viewable by @everyone"),
                    ),
            )
            .child(separator())
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(
                        div().child(heading_sm("Upload a custom thumbnail")).child(
                            text(12., 16., 500., "--text-muted")
                                .child("72x72 minimum. 1:1 aspect ratio. PNG, JPG"),
                        ),
                    )
                    .child(div().pr(px(8.)).child(thumb)),
            )
            .child(separator())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(Self::onboarding_form_title(
                        "This task is complete when:",
                        true,
                        false,
                    ))
                    .child(
                        div()
                            .pt(px(8.))
                            .pb(px(4.))
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child(radio(
                                "onboarding-task-view",
                                "Member visits the channel",
                                0,
                                cx,
                            ))
                            .child(radio("onboarding-task-chat", chat_label, 1, cx)),
                    )
                    .children(dialog.action_error.map(|message| {
                        text(12., 16., 400., "--text-feedback-critical").child(message)
                    })),
            )
            .into_any_element();
        let ready = !title.is_empty()
            && dialog.channel.is_some()
            && dialog.action.is_some()
            && title_error.is_none()
            && dialog.action_error.is_none();
        let remove = dialog.editing.as_ref().map(|_| {
            Self::onboarding_remove_button(
                "onboarding-task-remove",
                |this, cx| this.onboarding_remove_task(cx),
                cx,
            )
        });
        let _ = has_image;
        let footer = self.onboarding_footer_buttons(
            "onboarding-task",
            remove,
            ready,
            |this, cx| this.onboarding_save_task(cx),
            cx,
        );
        self.onboarding_dialog(
            "onboarding-task-dialog",
            "Edit task".into(),
            None,
            false,
            body,
            footer,
            cx,
        )
    }

    /// Module 983161.
    fn onboarding_resource_modal(
        &self,
        dialog: &ResourceDialog,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let guild = self.onboarding().map_or(0, |state| state.guild);
        let known = self.stores.guild(guild);
        let found = dialog.channel.as_ref().and_then(|id| {
            known.and_then(|known| known.channels.iter().find(|c| c.id.to_string() == *id))
        });
        let rules = self.onboarding_rules_channel(guild);
        let value = found.zip(known).map(|(channel, known)| {
            (
                Some(channel_glyph(known, channel, rules)),
                channel.name.clone(),
            )
        });
        let open = self
            .onboarding()
            .is_some_and(|state| state.dropdown == Some(Dropdown::Single));
        let title = dialog.title.text(cx);
        let picture =
            dialog
                .icon
                .as_ref()
                .zip(dialog.channel.as_ref())
                .and_then(|(icon, channel)| {
                    self.images
                        .get(&Self::resource_icon_url(&Resource {
                            channel_id: channel.clone(),
                            icon: Some(icon.clone()),
                            ..Resource::default()
                        })?)
                        .cloned()
                });
        let separator = || div().my(px(24.)).child(rule());
        let body = div()
            .py(px(12.))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(Self::onboarding_form_title(
                        "1. Choose a resource channel",
                        true,
                        false,
                    ))
                    .child(self.onboarding_select("onboarding-resource-channel", value, open, cx))
                    .child(
                        text(12., 16., 500., "--text-muted")
                            .child("You can only select channels that are readonly for @everyone."),
                    ),
            )
            .child(separator())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(Self::onboarding_form_title(
                        "2. Give this resource a name",
                        true,
                        false,
                    ))
                    .child(self.onboarding_input(&dialog.title.editor, false, 40., cx)),
            )
            .child(separator())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(Self::onboarding_form_title(
                        "3. Give this resource a description",
                        false,
                        false,
                    ))
                    .child(self.onboarding_text_area(
                        &dialog.description.editor,
                        "Ex: Rules for the server",
                        89.,
                        cx,
                    )),
            )
            .child(separator())
            .child(
                div()
                    .h(px(74.))
                    .flex()
                    .justify_between()
                    .child(
                        div()
                            .child(heading_sm("4. Upload a custom thumbnail"))
                            .child(
                                text(12., 16., 500., "--text-muted")
                                    .child("72x72 minimum. 1:1 aspect ratio. PNG, JPG"),
                            ),
                    )
                    .child(
                        div()
                            .id("onboarding-resource-thumb")
                            .relative()
                            .size(px(72.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(color("--background-base-lowest"))
                            .bg(color("--background-mod-normal"))
                            .cursor_pointer()
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(if let Some(picture) = picture {
                                img(picture).size(px(70.)).into_any_element()
                            } else {
                                icon("ob_upload", 24., "--interactive-text-default")
                                    .into_any_element()
                            })
                            .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                                if let Some(uri) = paths
                                    .paths()
                                    .first()
                                    .and_then(|path| super::server_settings::data_uri(path))
                                {
                                    this.onboarding_resource_upload(uri, cx);
                                }
                            }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.onboarding_pick_picture(
                                    |this, uri, cx| this.onboarding_resource_upload(uri, cx),
                                    cx,
                                );
                            })),
                    ),
            )
            .into_any_element();
        let ready = !title.is_empty() && dialog.channel.is_some();
        let remove = dialog.editing.as_ref().map(|_| {
            Self::onboarding_remove_button(
                "onboarding-resource-remove",
                |this, cx| this.onboarding_remove_resource(cx),
                cx,
            )
        });
        let footer = self.onboarding_footer_buttons(
            "onboarding-resource",
            remove,
            ready,
            |this, cx| this.onboarding_save_resource(cx),
            cx,
        );
        self.onboarding_dialog(
            "onboarding-resource-dialog",
            "Edit resource".into(),
            None,
            true,
            body,
            footer,
            cx,
        )
    }

    /// Module 147696.
    fn onboarding_unlock_modal(&self, dialog: &UnlockDialog, cx: &mut Context<Self>) -> AnyElement {
        let close = |this: &mut Root, _: &mut Window, _: &mut Context<Root>| {
            if let Some(state) = this.onboarding_mut() {
                state.dialog = None;
            }
        };
        if !dialog.reviewing {
            let line = |emoji: &'static str,
                        before: &'static str,
                        strong: &'static str,
                        after: &'static str| {
                let content = format!("{before}{strong}{after}");
                div()
                    .flex()
                    .gap(px(12.))
                    .child(text(16., 20., 400., "--text-muted").child(emoji))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-muted"))
                            .child(StyledText::new(content).with_highlights(vec![(
                                before.len()..before.len() + strong.len(),
                                HighlightStyle {
                                    color: Some(super::hsla("--text-strong")),
                                    font_weight: Some(FontWeight(500.)),
                                    ..HighlightStyle::default()
                                },
                            )])),
                    )
            };
            return modal::render(
                modal::Spec {
                    id: "onboarding-unlock-note",
                    title: "Allow @everyone".into(),
                    subtitle: None,
                    body: Some(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child(line(
                                "\u{1F440}",
                                "Default Channels need ",
                                "View Channel permission for @everyone.",
                                " Members can't be opted into channels they don't have access to.",
                            ))
                            .child(line(
                                "\u{1F929}",
                                "Letting your members view top channels will ",
                                "help them understand your community.",
                                "",
                            ))
                            .child(line(
                                "\u{1F4DA}",
                                "Don't worry, new members still need to ",
                                "agree to the rules",
                                " before chatting in your server.",
                            ))
                            .into_any_element(),
                    ),
                    buttons: vec![
                        modal::button("onboarding-unlock-cancel", "Cancel", Tone::Secondary, close),
                        modal::button(
                            "onboarding-unlock-review",
                            "Review Permissions",
                            Tone::Primary,
                            |this, _, _| {
                                if let Some(Dialog::Unlock(dialog)) = this
                                    .onboarding_mut()
                                    .and_then(|state| state.dialog.as_mut())
                                {
                                    dialog.reviewing = true;
                                }
                            },
                        ),
                    ],
                    close: Box::new(close),
                },
                cx,
            );
        }
        let guild = self.onboarding().map_or(0, |state| state.guild);
        let known = self.stores.guild(guild);
        let all_selected = dialog.selected.len() == dialog.channels.len();
        let count = dialog.channels.len();
        let mut rows = div().flex().flex_col();
        for id in &dialog.channels {
            let id = *id;
            let Some(channel) = known.and_then(|known| known.channels.iter().find(|c| c.id == id))
            else {
                continue;
            };
            let glyph = known.map_or("ob_text", |known| channel_glyph(known, channel, None));
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("onboarding-unlock-{id}")))
                    .h(px(44.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(icon(glyph, 16., "--icon-muted"))
                            .child(
                                text(16., 20., 500., "--text-default").child(channel.name.clone()),
                            ),
                    )
                    .child(checkbox(dialog.selected.contains(&id), false))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(Dialog::Unlock(dialog)) = this
                            .onboarding_mut()
                            .and_then(|state| state.dialog.as_mut())
                        {
                            if dialog.selected.contains(&id) {
                                dialog.selected.retain(|known| *known != id);
                            } else {
                                dialog.selected.push(id);
                            }
                        }
                        cx.notify();
                    })),
            );
        }
        let any = !dialog.selected.is_empty();
        modal::render(
            modal::Spec {
                id: "onboarding-unlock-review-dialog",
                title: "Update Channel Permissions".into(),
                subtitle: Some(
                    div()
                        .child("Select channels to allow everyone to either view this channel and/or connect to voice")
                        .into_any_element(),
                ),
                body: Some(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .h(px(28.))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(text(12., 16., 600., "--text-muted").child(if count == 1 {
                                    "1 Private Channel".to_owned()
                                } else {
                                    format!("{count} Private Channels")
                                }))
                                .child(
                                    div()
                                        .id("onboarding-unlock-all")
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(checkbox(all_selected, false))
                                        .child(text(14., 18., 400., "--text-default").child("Select All"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(Dialog::Unlock(dialog)) = this
                                                .onboarding_mut()
                                                .and_then(|state| state.dialog.as_mut())
                                            {
                                                dialog.selected = if all_selected {
                                                    Vec::new()
                                                } else {
                                                    dialog.channels.clone()
                                                };
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(rows)
                        .child(
                            div().mt(px(8.)).child(
                                text(12., 16., 400., "--text-muted")
                                    .child("@everyone role will be granted View Channel and Connect permissions."),
                            ),
                        )
                        .into_any_element(),
                ),
                buttons: vec![
                    modal::button("onboarding-unlock-back", "Cancel", Tone::Secondary, close),
                    modal::button(
                        "onboarding-unlock-update",
                        "Update Permissions",
                        if any { Tone::Primary } else { Tone::PrimaryDisabled },
                        |this, _, cx| this.onboarding_unlock_selected(cx),
                    ),
                ],
                close: Box::new(close),
            },
            cx,
        )
    }

    /// The open dropdown of a dialog: module 64749's list or module 890497's, or `set-image-for-action`.
    fn onboarding_dropdown(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.onboarding()?;
        let open = state.dropdown?;
        let guild = state.guild;
        let known = self.stores.guild(guild)?;
        let rules = self.onboarding_rules_channel(guild);
        let anchor = |key: &str| self.tip_anchors.borrow().get(key).copied();
        let (bounds, content): (gpui::Bounds<gpui::Pixels>, AnyElement) =
            match (open, state.dialog.as_ref()?) {
                (Dropdown::Channels, Dialog::Answer(dialog)) => {
                    let bounds = anchor("onboarding-answer-channels")?;
                    let query = dialog.channel_query.text(cx).trim().to_lowercase();
                    (
                        bounds,
                        self.onboarding_channel_results(known, dialog, &query, rules, cx),
                    )
                }
                (Dropdown::Roles, Dialog::Answer(dialog)) => {
                    let bounds = anchor("onboarding-answer-roles")?;
                    let query = dialog.role_query.text(cx).trim().to_lowercase();
                    (
                        bounds,
                        self.onboarding_role_results(known, dialog, &query, cx),
                    )
                }
                (Dropdown::Single, Dialog::Welcome(dialog)) => {
                    let bounds = anchor("onboarding-welcome-author")?;
                    let mut options: Vec<(String, String, Option<std::sync::Arc<gpui::Image>>)> =
                        Vec::new();
                    if let Some(me) = self.stores.current_user.as_ref() {
                        options.push((
                            me.id.to_string(),
                            me.display_name().to_owned(),
                            self.images.get(&avatar_source(me).0).cloned(),
                        ));
                    }
                    let selected = dialog.author.clone();
                    (
                        bounds,
                        self.onboarding_single_list(
                            options,
                            selected,
                            |this, id, cx| {
                                if let Some(Dialog::Welcome(dialog)) = this
                                    .onboarding_mut()
                                    .and_then(|state| state.dialog.as_mut())
                                {
                                    dialog.author = Some(id);
                                }
                                cx.notify();
                            },
                            cx,
                        ),
                    )
                }
                (Dropdown::Single, Dialog::Task(dialog)) => {
                    let bounds = anchor("onboarding-task-channel")?;
                    let taken: Vec<&String> = state
                        .home
                        .tasks
                        .iter()
                        .flatten()
                        .map(|task| &task.channel_id)
                        .filter(|id| dialog.editing.as_ref() != Some(*id))
                        .collect();
                    let options = selectable(known)
                        .into_iter()
                        .filter(|channel| {
                            !taken.contains(&&channel.id.to_string())
                                && task_channel(known, channel)
                        })
                        .map(|channel| (channel.id.to_string(), channel.name.clone(), None))
                        .collect();
                    let selected = dialog.channel.clone();
                    (
                        bounds,
                        self.onboarding_single_list(
                            options,
                            selected,
                            |this, id, cx| {
                                if let Some(Dialog::Task(dialog)) = this
                                    .onboarding_mut()
                                    .and_then(|state| state.dialog.as_mut())
                                {
                                    dialog.channel = Some(id);
                                }
                                this.onboarding_task_recheck();
                                cx.notify();
                            },
                            cx,
                        ),
                    )
                }
                (Dropdown::Single, Dialog::Resource(dialog)) => {
                    let bounds = anchor("onboarding-resource-channel")?;
                    let taken: Vec<&String> = state
                        .home
                        .resources
                        .iter()
                        .flatten()
                        .map(|resource| &resource.channel_id)
                        .collect();
                    let options = selectable(known)
                        .into_iter()
                        .filter(|channel| {
                            !taken.contains(&&channel.id.to_string())
                                && resource_channel(known, channel)
                        })
                        .map(|channel| (channel.id.to_string(), channel.name.clone(), None))
                        .collect();
                    let selected = dialog.channel.clone();
                    (
                        bounds,
                        self.onboarding_single_list(
                            options,
                            selected,
                            |this, id, cx| {
                                if let Some(Dialog::Resource(dialog)) = this
                                    .onboarding_mut()
                                    .and_then(|state| state.dialog.as_mut())
                                {
                                    dialog.channel = Some(id);
                                }
                                cx.notify();
                            },
                            cx,
                        ),
                    )
                }
                (Dropdown::ThumbMenu, Dialog::Task(dialog)) => {
                    let bounds = anchor("onboarding-task-thumb")?;
                    let has_image = dialog.icon.is_some()
                        || dialog.emoji.is_some()
                        || dialog.icon_key.is_some();
                    (bounds, self.onboarding_thumb_menu(has_image, cx))
                }
                _ => return None,
            };
        Some(
            deferred(
                div()
                    .id("onboarding-dropdown")
                    .occlude()
                    .absolute()
                    .left(bounds.left())
                    .top(bounds.bottom())
                    .child(content),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// `a.Ay`'s channel half (`d` of module 508595, in `resultsList_TCJuwN`).
    fn onboarding_channel_results(
        &self,
        known: &Guild,
        dialog: &AnswerDialog,
        query: &str,
        rules: Option<Id>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut top: Vec<&Channel> = known
            .channels
            .iter()
            .filter(|channel| channel.kind == ChannelKind::Category)
            .collect();
        let members: Vec<&Channel> = known
            .channels
            .iter()
            .filter(|channel| {
                (is_selectable(channel) || channel.kind == ChannelKind::Voice)
                    && !dialog.channels.contains(&channel.id.to_string())
                    && !is_private(known, channel)
            })
            .collect();
        top.extend(
            members
                .iter()
                .copied()
                .filter(|channel| channel.parent_id.is_none()),
        );
        top.sort_by(|left, right| {
            let category = |channel: &Channel| channel.kind == ChannelKind::Category;
            category(left)
                .cmp(&category(right))
                .then_with(|| left.position.cmp(&right.position))
        });
        let mut rows: Vec<(&Channel, bool)> = Vec::new();
        for channel in top {
            if dialog.channels.contains(&channel.id.to_string()) {
                continue;
            }
            rows.push((channel, false));
            if channel.kind == ChannelKind::Category {
                let mut under: Vec<&Channel> = members
                    .iter()
                    .copied()
                    .filter(|member| member.parent_id == Some(channel.id))
                    .collect();
                under.sort_by_key(|member| (is_vocal(member), member.position, member.id));
                rows.extend(under.into_iter().map(|member| (member, true)));
            }
        }
        rows.retain(|(channel, _)| query.is_empty() || channel.name.to_lowercase().contains(query));
        let mut list = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .when(!rows.is_empty(), |this| {
                this.child(
                    div()
                        .h(px(16.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child("Channels"),
                )
            });
        for (channel, nested) in rows {
            let id = channel.id.to_string();
            let glyph = if channel.kind == ChannelKind::Category {
                "automod_category"
            } else {
                channel_glyph(known, channel, rules)
            };
            list = list.child(
                div()
                    .id(SharedString::from(format!("onboarding-pick-{id}")))
                    .flex_none()
                    .h(px(26.))
                    .cursor_pointer()
                    .flex()
                    .child(
                        div()
                            .when(nested, |this| this.ml(px(12.)))
                            .p(px(4.))
                            .rounded(px(4.))
                            .bg(color("--interactive-background-hover"))
                            .flex()
                            .child(div().mr(px(4.)).child(icon(glyph, 16., "--text-muted")))
                            .child(body_sm_medium(channel.name.clone())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut query = None;
                        if let Some(state) = this.onboarding_mut() {
                            state.dropdown = None;
                            if let Some(Dialog::Answer(dialog)) = state.dialog.as_mut() {
                                dialog.channels.push(id.clone());
                                query = Some(dialog.channel_query.editor.clone());
                            }
                        }
                        if let Some(editor) = query {
                            editor.update(cx, |editor, _| editor.set_text(""));
                        }
                        cx.notify();
                    })),
            );
        }
        Self::onboarding_results_box(list)
    }

    /// `a.Ay`'s role half (`o` of module 57222).
    fn onboarding_role_results(
        &self,
        known: &Guild,
        dialog: &AnswerDialog,
        query: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut roles: Vec<(Id, String)> = known
            .roles
            .iter()
            .filter(|(id, name)| {
                **id != known.id
                    && !dialog.roles.contains(&id.to_string())
                    && (query.is_empty() || name.to_lowercase().contains(query))
            })
            .map(|(id, name)| (*id, name.clone()))
            .collect();
        roles.sort_by(|left, right| left.1.cmp(&right.1));
        let mut list = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .when(!roles.is_empty(), |this| {
                this.child(
                    div()
                        .h(px(16.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child("Roles"),
                )
            });
        for (id, name) in roles {
            let id = id.to_string();
            list = list.child(
                div()
                    .id(SharedString::from(format!("onboarding-pick-role-{id}")))
                    .flex_none()
                    .h(px(26.))
                    .cursor_pointer()
                    .flex()
                    .child(
                        div()
                            .p(px(4.))
                            .rounded(px(4.))
                            .bg(color("--interactive-background-hover"))
                            .flex()
                            .child(body_sm_medium(name)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut query = None;
                        if let Some(state) = this.onboarding_mut() {
                            state.dropdown = None;
                            if let Some(Dialog::Answer(dialog)) = state.dialog.as_mut() {
                                dialog.roles.push(id.clone());
                                query = Some(dialog.role_query.editor.clone());
                            }
                        }
                        if let Some(editor) = query {
                            editor.update(cx, |editor, _| editor.set_text(""));
                        }
                        cx.notify();
                    })),
            );
        }
        Self::onboarding_results_box(list)
    }

    fn onboarding_results_box(list: gpui::Div) -> AnyElement {
        div()
            .w(px(430.))
            .max_h(px(260.))
            .pt(px(16.))
            .pb(px(16.))
            .px(px(12.))
            .rounded_b(px(5.))
            .border_1()
            .border_color(color("--background-base-lowest"))
            .bg(color("--background-surface-high"))
            .shadow(shadow_high())
            .child(list)
            .into_any_element()
    }

    /// `listBox_b5PEJH`.
    fn onboarding_single_list(
        &self,
        options: Vec<(String, String, Option<std::sync::Arc<gpui::Image>>)>,
        selected: Option<String>,
        pick: impl Fn(&mut Self, String, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pick = std::rc::Rc::new(pick);
        let mut list = div()
            .mt(px(4.))
            .w(px(430.))
            .max_h(px(240.))
            .p(px(1.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(shadow_high())
            .flex()
            .flex_col();
        for (id, label, picture) in options {
            let picked = selected.as_ref() == Some(&id);
            let chosen = id.clone();
            let pick = pick.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("onboarding-option-{id}")))
                    .h(px(48.))
                    .px(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .children(picture.map(|picture| img(picture).size(px(20.)).rounded_full()))
                    .child(
                        text(16., 20., 400., "--text-default")
                            .flex_1()
                            .min_w(px(0.))
                            .child(label),
                    )
                    .when(picked, |this| {
                        this.child(icon("select_check", 20., "--text-brand"))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(state) = this.onboarding_mut() {
                            state.dropdown = None;
                        }
                        pick(this, chosen.clone(), cx);
                    })),
            );
        }
        list.into_any_element()
    }

    /// `set-image-for-action`.
    fn onboarding_thumb_menu(&self, has_image: bool, cx: &mut Context<Self>) -> AnyElement {
        let item =
            |id: &'static str, label: &'static str, glyph: &'static str, tint: &'static str| {
                div()
                    .id(id)
                    .h(px(36.))
                    .mx(px(8.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon(glyph, 20., tint))
                    .child(text(14., 18., 500., tint).child(label))
            };
        div()
            .mt(px(8.))
            .w(px(143.))
            .py(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(shadow_high())
            .flex()
            .flex_col()
            .child(
                item(
                    "onboarding-thumb-emoji",
                    "Choose Emoji",
                    "menu_add_reaction",
                    "--text-default",
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    if let Some(state) = this.onboarding_mut() {
                        state.dropdown = None;
                    }
                    let anchor = this
                        .tip_anchors
                        .borrow()
                        .get("onboarding-task-thumb")
                        .copied();
                    if let Some(bounds) = anchor {
                        this.open_picker(
                            super::picker::Target::OnboardingEmoji,
                            gpui::point(bounds.left() - px(300.), bounds.top()),
                            window,
                            cx,
                        );
                    }
                })),
            )
            .child(
                item(
                    "onboarding-thumb-upload",
                    "Upload Icon",
                    "ob_upload",
                    "--text-default",
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(state) = this.onboarding_mut() {
                        state.dropdown = None;
                    }
                    this.onboarding_pick_picture(
                        |this, uri, cx| this.onboarding_task_picture(uri, cx),
                        cx,
                    );
                })),
            )
            .when(has_image, |this| {
                this.child(
                    item(
                        "onboarding-thumb-clear",
                        "Remove Image",
                        "ob_trash",
                        "--text-feedback-critical",
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this.onboarding_mut() {
                            state.dropdown = None;
                            if let Some(Dialog::Task(dialog)) = state.dialog.as_mut() {
                                dialog.icon = None;
                                dialog.icon_key = None;
                                dialog.emoji = None;
                            }
                        }
                        cx.notify();
                    })),
                )
            })
            .into_any_element()
    }

    /// The sidebar entry's trailing text.
    pub(super) fn onboarding_decoration(&self, guild: Id) -> Option<String> {
        let state = self.onboarding()?;
        decoration(
            state.saved.enabled,
            ever_enabled(self.stores.guild(guild)),
            state.step,
        )
    }

    /// `predicate`: `i_.rs`.
    pub(super) fn onboarding_allowed(&self, guild: Id) -> bool {
        let Some((known, me)) = self.stores.guild(guild).zip(self.stores.me()) else {
            return false;
        };
        has_feature(Some(known), "COMMUNITY")
            && permissions::can_in_guild(known, me, permissions::MANAGE_GUILD)
            && permissions::can_in_guild(known, me, permissions::MANAGE_ROLES)
    }
}

/// `gz`'s `status`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    None,
    Good,
    Warning,
    Required,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild(everyone: u64, channels: Vec<Channel>) -> Guild {
        Guild {
            id: 1,
            name: String::new(),
            icon: None,
            owner_id: None,
            home_header: None,
            rules_channel_id: None,
            joined_at: String::new(),
            default_message_notifications: 0,
            nsfw_level: 0,
            channels,
            roles: HashMap::from([(1, "@everyone".to_owned())]),
            role_permissions: HashMap::from([(1, everyone)]),
            role_info: HashMap::new(),
            my_roles: Vec::new(),
            my_pending: false,
            my_flags: 0,
            my_timeout_until: None,
            mfa_level: 0,
            vanity_url_code: None,
            invites_disabled_until: None,
            dms_disabled_until: None,
            lockdown_duration_hours: None,
            features: Vec::new(),
            premium_tier: 0,
            premium_subscription_count: 0,
            emojis: Vec::new(),
            threads: Vec::new(),
        }
    }

    fn channel(id: Id, kind: ChannelKind, parent: Option<Id>, position: i64) -> Channel {
        Channel {
            id,
            name: format!("c{id}"),
            kind,
            position,
            parent_id: parent,
            last_message_id: None,
            overwrites: Vec::new(),
            topic: None,
            rate_limit_per_user: 0,
            user_limit: 0,
            flags: 0,
            default_auto_archive_duration: None,
            nsfw: false,
            available_tags: Vec::new(),
            default_sort_order: None,
            default_forum_layout: 0,
        }
    }

    fn deny(mut channel: Channel, bits: u64) -> Channel {
        channel.overwrites.push(permissions::Overwrite {
            id: 1,
            kind: 0,
            allow: 0,
            deny: bits,
        });
        channel
    }

    fn answer(id: &str, channels: &[&str]) -> Answer {
        Answer {
            id: id.to_owned(),
            title: id.to_owned(),
            channel_ids: channels.iter().map(ToString::to_string).collect(),
            ..Answer::default()
        }
    }

    fn known_all() -> (impl Fn(&str) -> bool, impl Fn(&str) -> bool) {
        (|_: &str| true, |_: &str| true)
    }

    #[test]
    fn the_wizard_walks_uf_kd_in_order() {
        assert_eq!(next_step(Step::Landing), Some(Step::Safety));
        assert_eq!(next_step(Step::Questions), Some(Step::Home));
        assert_eq!(next_step(Step::Home), Some(Step::Review));
        assert_eq!(next_step(Step::Review), None);
        assert_eq!(previous_step(Step::Safety), Some(Step::Landing));
        assert_eq!(previous_step(Step::Landing), None);
    }

    #[test]
    fn the_decoration_says_on_off_or_the_step() {
        assert_eq!(decoration(true, true, Step::Review).as_deref(), Some("ON"));
        assert_eq!(
            decoration(false, true, Step::Landing).as_deref(),
            Some("OFF")
        );
        assert_eq!(decoration(false, false, Step::Landing), None);
        assert_eq!(
            decoration(false, false, Step::Safety).as_deref(),
            Some("1 of 5")
        );
        assert_eq!(
            decoration(false, false, Step::Review).as_deref(),
            Some("5 of 5")
        );
    }

    #[test]
    fn prompts_cross_the_wire_like_sa_and_i() {
        let wire = json!({
            "id": "7", "title": "Q", "single_select": true, "required": true, "in_onboarding": true, "type": 1,
            "options": [{"id": "8", "title": "A", "description": "d", "channel_ids": ["9"], "role_ids": [],
                          "emoji": {"id": null, "name": "x", "animated": false}}],
        });
        let prompt = Prompt::from_wire(&wire);
        assert_eq!(prompt.options[0].channel_ids, vec!["9"]);
        let sent = prompt.to_wire();
        assert_eq!(sent["single_select"], true);
        assert_eq!(sent["options"][0]["emoji_name"], "x");
        assert_eq!(sent["options"][0]["emoji_id"], Value::Null);
        assert_eq!(sent["options"][0]["channel_ids"][0], "9");
    }

    #[test]
    fn the_starter_is_only_the_starter_while_untouched() {
        let mut prompt = Prompt::starter("5".to_owned());
        assert!(prompt.is_starter());
        prompt.required = true;
        assert!(!prompt.is_starter());
        assert!(!Prompt::blank("6".to_owned(), true).is_starter());
    }

    #[test]
    fn validation_finds_what_h_finds() {
        let (role, channel) = known_all();
        let known = Known {
            role: &role,
            channel: &channel,
        };
        let empty = Prompt::blank("1".to_owned(), true);
        let error = prompt_error(&known, std::slice::from_ref(&empty), &empty).unwrap();
        assert_eq!(error.title, Some("Question is required"));
        assert_eq!(error.options, Some("At least one answer is required"));
        let mut good = Prompt::starter("2".to_owned());
        good.options.push(answer("a", &["9"]));
        assert_eq!(
            prompt_error(&known, std::slice::from_ref(&good), &good),
            None
        );
        good.options.push(answer("b", &[]));
        assert_eq!(
            prompt_error(&known, std::slice::from_ref(&good), &good)
                .unwrap()
                .option_errors[1],
            Some(NEEDS_CHANNEL_OR_ROLE)
        );
    }

    #[test]
    fn a_single_select_role_cannot_come_from_another_question() {
        let (role, channel) = known_all();
        let known = Known {
            role: &role,
            channel: &channel,
        };
        let mut first = Prompt::starter("1".to_owned());
        first.single_select = true;
        let mut mine = answer("a", &[]);
        mine.role_ids = vec!["5".to_owned()];
        first.options.push(mine.clone());
        let mut second = Prompt::starter("2".to_owned());
        second.options.push(mine.clone());
        let prompts = vec![first.clone(), second];
        assert_eq!(
            answer_error(&known, &prompts, &first, &mine),
            Some(ROLE_CONFLICT)
        );
    }

    #[test]
    fn a_save_drops_what_no_longer_exists_and_keeps_order() {
        let role = |_: &str| false;
        let channel = |id: &str| id == "9";
        let known = Known {
            role: &role,
            channel: &channel,
        };
        let mut after = Prompt::starter("1".to_owned());
        after.in_onboarding = false;
        after.options.push(answer("a", &["9", "404"]));
        let mut before = Prompt::starter("2".to_owned());
        before.options.push(answer("b", &["9"]));
        let list = save_list(&known, &[], &[after, before], false, &|_| true);
        let SaveList::Send(list) = list else {
            panic!("should send");
        };
        assert_eq!(list[0].id, "2");
        assert_eq!(list[1].options[0].channel_ids, vec!["9"]);
    }

    #[test]
    fn a_save_with_no_questions_left_sends_an_empty_list() {
        let (role, channel) = known_all();
        let known = Known {
            role: &role,
            channel: &channel,
        };
        let saved = vec![Prompt::starter("1".to_owned())];
        assert!(matches!(
            save_list(&known, &saved, &[], false, &|_| true),
            SaveList::Send(list) if list.is_empty()
        ));
        assert!(matches!(
            save_list(&known, &saved, &saved, false, &|_| true),
            SaveList::Nothing
        ));
    }

    #[test]
    fn a_save_with_too_many_prejoin_questions_stops() {
        let (role, channel) = known_all();
        let known = Known {
            role: &role,
            channel: &channel,
        };
        let many: Vec<Prompt> = (0..5)
            .map(|index| {
                let mut prompt = Prompt::starter(index.to_string());
                prompt.options.push(answer("a", &["9"]));
                prompt
            })
            .collect();
        assert!(matches!(
            save_list(&known, &[], &many, false, &|_| true),
            SaveList::Invalid(_)
        ));
    }

    #[test]
    fn the_separator_decides_which_side_a_question_lands_on() {
        let mut first = Prompt::starter("a".to_owned());
        first.required = true;
        let second = Prompt::blank("b".to_owned(), false);
        let list = move_prompt(&[first, second], 0, 2).unwrap();
        assert_eq!(list[0].id, "b");
        assert!(!list[1].in_onboarding);
        assert!(!list[1].required);
        let prompts: Vec<Prompt> = (0..4)
            .map(|index| Prompt::starter(index.to_string()))
            .chain([Prompt::blank("x".to_owned(), false)])
            .collect();
        assert_eq!(move_prompt(&prompts, 5, 0), None);
    }

    #[test]
    fn everyone_follows_the_overwrite_as_mj_does() {
        let open = channel(10, ChannelKind::Text, None, 0);
        let hidden = deny(channel(11, ChannelKind::Text, None, 1), VIEW);
        let readonly = deny(channel(12, ChannelKind::Text, None, 2), SEND);
        let value = guild(
            VIEW | SEND,
            vec![open.clone(), hidden.clone(), readonly.clone()],
        );
        assert!(can_default(&value, &open));
        assert!(!can_default(&value, &hidden));
        assert!(is_private(&value, &hidden));
        assert!(!is_chattable(&value, &readonly));
        assert!(resource_channel(&value, &readonly));
        assert!(!resource_channel(&value, &open));
        assert!(task_channel(&value, &readonly));
        assert!(can_complete(&value, &readonly, 0));
        assert!(!can_complete(&value, &readonly, 1));
    }

    #[test]
    fn a_followed_category_brings_its_channels() {
        let value = guild(
            VIEW | SEND | CONNECT,
            vec![
                channel(20, ChannelKind::Category, None, 0),
                channel(21, ChannelKind::Text, Some(20), 0),
                channel(22, ChannelKind::Text, Some(20), 1),
                channel(23, ChannelKind::Voice, Some(20), 2),
                channel(24, ChannelKind::Text, None, 0),
            ],
        );
        let (chattable, all) = default_reach(&value, &["20".to_owned()]);
        assert_eq!(all, vec![21, 22]);
        assert_eq!(chattable, vec![21, 22]);
        let (covered, missing) = coverage(&value, &["20".to_owned()], &[]);
        assert_eq!(covered.len(), 2);
        assert_eq!(missing.len(), 1);
        let ids: BTreeSet<String> = ["20".to_owned()].into_iter().collect();
        let rows = sidebar_rows(&value, &ids);
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![20, 21, 22, 23]
        );
    }

    #[test]
    fn required_questions_count_their_fewest_channels() {
        let value = guild(
            VIEW | SEND,
            vec![
                channel(30, ChannelKind::Text, None, 0),
                channel(31, ChannelKind::Text, None, 1),
                channel(32, ChannelKind::Text, None, 2),
            ],
        );
        let mut prompt = Prompt::starter("q".to_owned());
        prompt.required = true;
        prompt.options.push(answer("a", &["31", "32"]));
        prompt.options.push(answer("b", &["32"]));
        let total = required_reach(&value, &["30".to_owned()], &[prompt], false);
        assert_eq!(total, vec![30, 32]);
    }

    #[test]
    fn the_browser_lists_uncategorized_first_and_voice_last() {
        let value = guild(
            VIEW | SEND,
            vec![
                channel(40, ChannelKind::Category, None, 1),
                channel(41, ChannelKind::Voice, Some(40), 0),
                channel(42, ChannelKind::Text, Some(40), 5),
                channel(43, ChannelKind::Text, None, 9),
                channel(44, ChannelKind::Category, None, 0),
            ],
        );
        let groups = browser_groups(&value);
        assert_eq!(groups.len(), 2);
        assert!(groups[0].category.is_none());
        assert_eq!(groups[1].channels[0].id, 42);
        assert_eq!(groups[1].channels[1].id, 41);
    }

    #[test]
    fn recommended_skips_the_rules_channel_and_a_third_announcement() {
        let value = guild(
            VIEW | SEND,
            vec![
                channel(50, ChannelKind::Other(5), None, 0),
                channel(51, ChannelKind::Other(5), None, 1),
                channel(52, ChannelKind::Other(5), None, 2),
                channel(53, ChannelKind::Text, None, 3),
                deny(channel(54, ChannelKind::Text, None, 4), VIEW),
            ],
        );
        let top: Vec<String> = ["54", "50", "51", "52", "53"].map(str::to_owned).to_vec();
        let list = recommended(&value, &top, Some(53));
        assert_eq!(
            list.iter().map(|channel| channel.id).collect::<Vec<_>>(),
            vec![50, 51, 54]
        );
    }

    #[test]
    fn the_guide_validates_like_ic() {
        let viewable = |id: &str| id == "9";
        let mut home = Home::default();
        assert!(home.is_empty());
        assert!(home.is_valid(&viewable));
        home.welcome = Some(Welcome {
            author_ids: vec!["1".to_owned()],
            message: "short".to_owned(),
        });
        assert!(!home.is_valid(&viewable));
        home.welcome = Some(Welcome {
            author_ids: vec!["1".to_owned()],
            message: "long enough".to_owned(),
        });
        assert!(home.is_valid(&viewable));
        home.tasks = Some(vec![Task {
            channel_id: "404".to_owned(),
            ..Task::default()
        }]);
        assert!(!home.is_valid(&viewable));
    }

    #[test]
    fn the_guide_crosses_the_wire_like_y4_and_xu() {
        let channel = |id: &str| id == "9";
        let home = Home {
            welcome: Some(Welcome {
                author_ids: vec!["1".to_owned()],
                message: "welcome all".to_owned(),
            }),
            tasks: Some(vec![
                Task {
                    channel_id: "9".to_owned(),
                    title: "Say hello".to_owned(),
                    emoji: Some(Emoji {
                        id: None,
                        name: Some("x".to_owned()),
                        animated: None,
                    }),
                    ..Task::default()
                },
                Task {
                    channel_id: "404".to_owned(),
                    ..Task::default()
                },
            ]),
            resources: None,
            enabled: true,
        };
        let wire = home.to_wire(7, &channel);
        assert_eq!(wire["guild_id"], "7");
        assert_eq!(wire["new_member_actions"].as_array().unwrap().len(), 1);
        assert_eq!(wire["new_member_actions"][0]["emoji"]["name"], "x");
        assert_eq!(wire["resource_channels"], json!([]));
        let back = Home::from_wire(&wire, &channel);
        assert_eq!(back.tasks.as_ref().unwrap()[0].title, "Say hello");
        assert!(back.enabled);
        assert_eq!(Home::from_wire(&Value::Null, &channel), Home::default());
    }

    #[test]
    fn the_welcome_dialog_says_what_is_missing() {
        assert_eq!(welcome_error(false, ""), None);
        assert_eq!(
            welcome_error(false, "hi").as_deref(),
            Some("Welcome messages require both an author and a message of at least 7 characters.")
        );
        assert_eq!(
            welcome_error(false, "long enough").as_deref(),
            Some("Welcome messages require an author")
        );
        assert_eq!(
            welcome_error(true, "hi").as_deref(),
            Some("Welcome messages require a message of at least 7 characters.")
        );
        assert_eq!(welcome_error(true, "long enough"), None);
    }

    #[test]
    fn errors_come_out_with_their_field() {
        let body = json!({"code": 50035, "errors": {"default_channel_ids": {"_errors": [{"code": "X", "message": "Too few"}]}}, "message": "Invalid Form Body"});
        assert_eq!(field_error(&body), "default_channel_ids: Too few");
        assert_eq!(
            field_error(&json!({"message": "Missing Access"})),
            "message: Missing Access".replace("message: ", "")
        );
        assert_eq!(
            guide_error(&json!({"welcome_message": ["Too short"]})),
            "Too short"
        );
    }

    #[test]
    fn unlocking_clears_the_access_bits_only() {
        let mut locked = channel(60, ChannelKind::Text, None, 0);
        locked.overwrites.push(permissions::Overwrite {
            id: 1,
            kind: 0,
            allow: SEND,
            deny: VIEW | 1,
        });
        let body = unlocked_overwrite(1, &locked);
        assert_eq!(body["deny"], "1");
        assert_eq!(body["allow"], SEND.to_string());
        assert_eq!(body["id"], "1");
    }

    #[test]
    fn advanced_mode_counts_questions_on_the_channels_step() {
        let mut state = OnboardingState::new(1);
        state.step = Step::Channels;
        state.prompts.push(Prompt::starter("1".to_owned()));
        assert!(!state.step_changed());
        state.saved.mode = 1;
        assert!(state.step_changed());
        state.step = Step::Questions;
        assert!(state.step_configured());
        assert_eq!(state.upsell(), Some("Customization Questions"));
        state.upsells_dismissed.insert("Customization Questions");
        assert_eq!(state.upsell(), None);
    }

    #[test]
    fn typing_stops_at_max_length() {
        assert_eq!(clipped("abcdef", 3).as_deref(), Some("abc"));
        assert_eq!(clipped("abc", 3), None);
    }
}
