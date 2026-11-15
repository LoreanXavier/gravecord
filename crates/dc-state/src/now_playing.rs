//! NowPlayingViewStore's party grouping (943577), fed by presence, voice and cached applications.
use crate::{Id, RELATIONSHIP_FRIEND, Stores, permissions};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Activity {
    pub application: Application,
    pub value: Value,
    pub user: Id,
    pub members: Vec<Id>,
    pub started: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceChannel {
    pub guild: Id,
    pub channel: Id,
    pub members: Vec<Id>,
    pub all_hidden: bool,
    pub any_hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stream {
    pub guild: Id,
    pub channel: Id,
    pub owner: Id,
}

#[derive(Clone, Debug)]
pub struct Party {
    pub id: String,
    pub priority: Vec<Id>,
    pub members: Vec<Id>,
    pub activities: Vec<Activity>,
    pub voice: Vec<VoiceChannel>,
    pub streams: Vec<Stream>,
    pub guild_context: Option<Id>,
    pub spotify: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Affinity {
    pub user: Id,
    pub communication_rank: u64,
    pub communication_probability: f64,
    pub voice_probability: f64,
}

/// Experiment 2025-08-voice-user-affinity; default retains the relationship store's order.
#[derive(Clone, Copy, Default)]
pub enum Sort {
    #[default]
    Default,
    VoiceProbability,
    CommunicationProbability,
}

#[derive(Default)]
pub struct Feed {
    pub parties: Vec<Party>,
    pub missing_applications: BTreeSet<String>,
}

pub fn affinities(body: &Value) -> Vec<Affinity> {
    body.get("user_affinities")
        .or_else(|| body.get("userAffinities"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some(Affinity {
                user: row
                    .get("other_user_id")
                    .or_else(|| row.get("otherUserId"))?
                    .as_str()?
                    .parse()
                    .ok()?,
                communication_rank: row
                    .get("communication_rank")
                    .or_else(|| row.get("communicationRank"))
                    .and_then(Value::as_u64)
                    .unwrap_or(u64::MAX),
                communication_probability: row
                    .get("communication_probability")
                    .or_else(|| row.get("communicationProbability"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.),
                voice_probability: row
                    .get("vc_probability")
                    .or_else(|| row.get("vcProbability"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.),
            })
        })
        .collect()
}

fn current_activity(stores: &Stores, user: Id) -> Option<&Value> {
    stores
        .presence
        .get(user, None)?
        .activities
        .iter()
        .find(|value| value["type"] != 4)
}

pub fn is_spotify(activity: &Value) -> bool {
    activity["type"] == 2
        && activity["name"] == "Spotify"
        && activity["party"]["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("spotify:"))
}

/// Application key of 294857, including Spotify, Xbox and Twitch fallback applications.
pub fn application_key(activity: &Value) -> Option<String> {
    if is_spotify(activity) {
        return Some("spotify".into());
    }
    if activity["type"] == 1
        && let Some(url) = activity["url"].as_str()
        && [
            "https://twitch.tv/",
            "https://www.twitch.tv/",
            "http://twitch.tv/",
            "http://www.twitch.tv/",
            "https://youtube.com/watch?v=",
            "http://youtube.com/watch?v=",
            "https://www.youtube.com/watch?v=",
            "http://www.youtube.com/watch?v=",
        ]
        .iter()
        .any(|prefix| url.starts_with(prefix))
    {
        return Some(format!("twitch:{url}"));
    }
    let app = activity["application_id"].as_str();
    if let Some(id) = app.filter(|id| *id != "438122941302046720") {
        return Some(id.to_owned());
    }
    (app == Some("438122941302046720") || activity["platform"] == "xbox")
        .then(|| format!("xbox:{}", activity["name"].as_str().unwrap_or_default()))
}

fn visible_voice(stores: &Stores, user: Id) -> Option<(Id, Id)> {
    let me = stores.me()?;
    stores.voice_states.iter().find_map(|(&guild_id, states)| {
        let state = states.iter().find(|state| state.user_id == user)?;
        if guild_id == 0 {
            return stores
                .private_channels
                .iter()
                .any(|channel| channel.id == state.channel_id)
                .then_some((0, state.channel_id));
        }
        let guild = stores.guild(guild_id)?;
        let channel = guild
            .channels
            .iter()
            .find(|channel| channel.id == state.channel_id)?;
        permissions::can_in_channel(guild, channel, me, permissions::VIEW_CHANNEL)
            .then_some((guild_id, channel.id))
    })
}

/// Rebuilds from the stores after presence/voice/relationship changes; it never joins or writes settings.
pub fn feed(
    stores: &Stores,
    affinities: &[Affinity],
    sort: Sort,
    application: impl Fn(&str) -> Option<Application>,
) -> Feed {
    let mut out = Feed::default();
    let mut priority: BTreeSet<Id> = stores
        .relationships
        .iter()
        .filter_map(|(&id, &kind)| (kind == RELATIONSHIP_FRIEND).then_some(id))
        .collect();
    if stores.consents.get("personalization") == Some(&true) {
        priority.extend(
            affinities
                .iter()
                .filter(|row| row.communication_rank <= 15)
                .map(|row| row.user),
        );
    }
    let blocked = |id: Id| stores.is_ignored(id) || stores.relationships.get(&id) == Some(&2);
    priority.retain(|id| {
        stores.users.get(id).is_some_and(|user| !user.bot)
            && stores.current_user.as_ref().is_none_or(|me| me.id != *id)
    });
    let mut groups: BTreeMap<String, Vec<Id>> = BTreeMap::new();
    for &user in &priority {
        let key = if let Some((_, channel)) = visible_voice(stores, user) {
            format!("channel-{channel}")
        } else if let Some(party) =
            current_activity(stores, user).and_then(|activity| activity["party"]["id"].as_str())
        {
            format!("party-{party}")
        } else {
            format!("user-{user}")
        };
        groups.entry(key).or_default().push(user);
    }
    let score = |id: Id| {
        let affinity = affinities.iter().find(|row| row.user == id);
        match sort {
            Sort::Default => 0.,
            Sort::VoiceProbability => affinity.map_or(0., |row| row.voice_probability),
            Sort::CommunicationProbability => {
                affinity.map_or(0., |row| row.communication_probability)
            }
        }
    };
    for (id, mut members) in groups {
        members.sort_by(|a, b| score(*b).total_cmp(&score(*a)));
        let mut party = Party {
            id,
            priority: members.clone(),
            members: members.clone(),
            activities: Vec::new(),
            voice: Vec::new(),
            streams: Vec::new(),
            guild_context: None,
            spotify: false,
        };
        let mut seen_apps = BTreeSet::new();
        for &user in &members {
            if let Some(stream) = stores.voice_states.iter().find_map(|(&guild, states)| {
                let state = states
                    .iter()
                    .find(|state| state.user_id == user && state.self_stream)?;
                Some(Stream {
                    guild,
                    channel: state.channel_id,
                    owner: user,
                })
            }) {
                party.streams.push(stream);
            }
            let Some(value) = current_activity(stores, user) else {
                continue;
            };
            let Some(key) = application_key(value) else {
                continue;
            };
            party.spotify = key == "spotify";
            let app = if key == "spotify" {
                Some(Application {
                    id: key.clone(),
                    name: "Spotify".into(),
                    icon: None,
                })
            } else if let Some(name) = key.strip_prefix("xbox:") {
                Some(Application {
                    id: key.clone(),
                    name: name.into(),
                    icon: None,
                })
            } else {
                application(&key)
            };
            let Some(app) = app else {
                if key.parse::<Id>().is_ok() {
                    out.missing_applications.insert(key);
                }
                continue;
            };
            if !seen_apps.insert(key.clone()) {
                continue;
            }
            let mut playing: Vec<Id> = if let Some(party_id) = value["party"]["id"].as_str() {
                stores
                    .users
                    .keys()
                    .copied()
                    .filter(|id| {
                        current_activity(stores, *id).is_some_and(|activity| {
                            activity["party"]["id"].as_str() == Some(party_id)
                        })
                    })
                    .collect()
            } else {
                members
                    .iter()
                    .copied()
                    .filter(|id| {
                        current_activity(stores, *id)
                            .and_then(application_key)
                            .as_deref()
                            == Some(key.as_str())
                    })
                    .collect()
            };
            playing.sort_by_key(|id| {
                (
                    stores.relationships.get(id) != Some(&RELATIONSHIP_FRIEND),
                    *id,
                )
            });
            party.activities.push(Activity {
                application: app,
                value: value.clone(),
                user,
                members: playing,
                started: crate::id(&value["timestamps"]["start"])
                    .or_else(|| crate::id(&value["created_at"])),
            });
        }
        let mut seen_channels = BTreeSet::new();
        for user in members.drain(..) {
            let Some((guild, channel)) = visible_voice(stores, user) else {
                continue;
            };
            if guild == 0 {
                continue;
            }
            if stores.afk_channels.get(&guild) == Some(&channel) || !seen_channels.insert(channel) {
                continue;
            }
            let states = stores.voice_members(guild, channel);
            let mut voice_members: Vec<Id> = states
                .iter()
                .map(|state| state.user_id)
                .filter(|id| stores.users.contains_key(id))
                .collect();
            voice_members.sort_by(|a, b| match sort {
                Sort::Default => (
                    stores.relationships.get(a) != Some(&RELATIONSHIP_FRIEND),
                    *a,
                )
                    .cmp(&(
                        stores.relationships.get(b) != Some(&RELATIONSHIP_FRIEND),
                        *b,
                    )),
                _ => score(*b).total_cmp(&score(*a)),
            });
            for &member in &voice_members {
                if !party.members.contains(&member) {
                    party.members.push(member);
                }
            }
            party.voice.push(VoiceChannel {
                guild,
                channel,
                members: voice_members,
                all_hidden: !states.is_empty()
                    && states.iter().all(|state| state.discoverable == Some(false)),
                any_hidden: states.iter().any(|state| state.discoverable == Some(false)),
            });
        }
        if party.members.iter().any(|id| blocked(*id))
            || (!party.voice.is_empty() && party.voice.iter().all(|voice| voice.all_hidden))
            || (party.voice.is_empty() && party.activities.is_empty() && party.streams.is_empty())
        {
            continue;
        }
        if party.priority.len() > 1
            && let Some(first) = party.voice.first()
            && party.voice.iter().all(|voice| voice.guild == first.guild)
            && party
                .priority
                .iter()
                .all(|id| visible_voice(stores, *id).is_some())
        {
            party.guild_context = Some(first.guild);
        }
        party.activities.sort_by(|a, b| {
            b.started
                .unwrap_or(0)
                .cmp(&a.started.unwrap_or(0))
                .then_with(|| a.application.name.cmp(&b.application.name))
        });
        out.parties.push(party);
    }
    let name = |party: &Party| {
        party
            .priority
            .iter()
            .filter_map(|id| stores.users.get(id))
            .map(|user| user.username.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ")
    };
    out.parties.sort_by_key(|party| {
        (
            party.voice.iter().any(|voice| voice.any_hidden),
            party.members.len() <= 1,
            party.streams.is_empty(),
            party.voice.is_empty(),
            party.activities.is_empty(),
            party.spotify,
            name(party),
        )
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stores() -> Stores {
        let mut stores = Stores::default();
        stores.dispatch("READY", &json!({"user":{"id":"1","username":"self"},
            "users":[{"id":"2","username":"Ada"},{"id":"3","username":"Bea"},{"id":"4","username":"Cal"}],
            "guilds":[{"id":"10","name":"fixture","owner_id":"1","channels":[
                {"id":"11","name":"voice","type":2},{"id":"12","name":"other","type":2}]}],
            "relationships":[{"id":"2","type":1},{"id":"3","type":1}],"private_channels":[]}));
        stores
    }

    fn activity(stores: &mut Stores, user: Id, value: Value) {
        stores.dispatch(
            "PRESENCE_UPDATE",
            &json!({"user":{"id":user.to_string()},"status":"online","activities":[value]}),
        );
    }

    fn game(name: &str, application: &str) -> Value {
        json!({"name":name,"application_id":application,"type":0,"timestamps":{"start":1000}})
    }

    fn application(id: &str) -> Option<Application> {
        Some(Application {
            id: id.into(),
            name: format!("Game {id}"),
            icon: None,
        })
    }

    fn voice(stores: &mut Stores, user: Id, stream: bool, discoverable: bool) {
        stores.dispatch("VOICE_STATE_UPDATE", &json!({"guild_id":"10","channel_id":"11",
            "user_id":user.to_string(),"session_id":"fixture","self_stream":stream,"discoverable":discoverable}));
    }

    #[test]
    fn friends_in_one_voice_channel_share_a_card_with_the_stream_and_both_games() {
        let mut stores = stores();
        activity(&mut stores, 2, game("one", "101"));
        activity(&mut stores, 3, game("two", "102"));
        voice(&mut stores, 2, true, true);
        voice(&mut stores, 3, false, true);
        voice(&mut stores, 4, false, true);
        let feed = feed(&stores, &[], Sort::Default, application);
        assert_eq!(feed.parties.len(), 1);
        let card = &feed.parties[0];
        assert_eq!(card.priority, [2, 3]);
        assert_eq!(card.members, [2, 3, 4]);
        assert_eq!(card.activities.len(), 2);
        assert_eq!(
            card.streams,
            [Stream {
                guild: 10,
                channel: 11,
                owner: 2
            }]
        );
        assert_eq!(card.voice[0].members, [2, 3, 4]);
        assert_eq!(card.guild_context, Some(10));
    }

    #[test]
    fn private_voice_groups_and_groups_containing_blocked_people_are_not_shown() {
        let mut stores = stores();
        activity(&mut stores, 2, game("one", "101"));
        voice(&mut stores, 2, true, false);
        voice(&mut stores, 3, false, false);
        assert!(
            feed(&stores, &[], Sort::Default, application)
                .parties
                .is_empty()
        );
        voice(&mut stores, 3, false, true);
        assert_eq!(
            feed(&stores, &[], Sort::Default, application).parties.len(),
            1
        );
        voice(&mut stores, 4, false, true);
        stores.relationships.insert(4, 2);
        assert!(
            feed(&stores, &[], Sort::Default, application)
                .parties
                .is_empty()
        );
    }

    #[test]
    fn affinity_people_require_consent_and_rank_fifteen_or_better() {
        let mut stores = stores();
        activity(&mut stores, 4, game("one", "101"));
        let mut affinity = Affinity {
            user: 4,
            communication_rank: 15,
            ..Affinity::default()
        };
        assert!(
            feed(&stores, &[affinity], Sort::Default, application)
                .parties
                .is_empty()
        );
        stores.consents.insert("personalization".into(), true);
        assert_eq!(
            feed(&stores, &[affinity], Sort::Default, application).parties[0].members,
            [4]
        );
        affinity.communication_rank = 16;
        assert!(
            feed(&stores, &[affinity], Sort::Default, application)
                .parties
                .is_empty()
        );
    }

    #[test]
    fn spotify_party_combines_listeners_without_an_application_fetch() {
        let mut stores = stores();
        let music = json!({"type":2,"name":"Spotify","party":{"id":"spotify:session"},"details":"Track","state":"Artist"});
        activity(&mut stores, 2, music.clone());
        activity(&mut stores, 3, music);
        let feed = feed(&stores, &[], Sort::Default, |_| None);
        assert_eq!(feed.parties.len(), 1);
        assert_eq!(feed.parties[0].activities.len(), 1);
        assert_eq!(feed.parties[0].activities[0].members, [2, 3]);
        assert!(feed.parties[0].spotify);
        assert!(feed.missing_applications.is_empty());
    }

    #[test]
    fn application_fetch_does_not_hide_an_existing_stream_or_voice_channel() {
        let mut stores = stores();
        activity(&mut stores, 2, game("one", "101"));
        voice(&mut stores, 2, true, true);
        let feed = feed(&stores, &[], Sort::Default, |_| None);
        assert_eq!(
            feed.missing_applications.into_iter().collect::<Vec<_>>(),
            ["101"]
        );
        assert_eq!(feed.parties.len(), 1);
        assert!(feed.parties[0].activities.is_empty());
        assert_eq!(feed.parties[0].streams.len(), 1);
    }
    #[test]
    fn cached_and_network_affinities_keep_the_same_ranking() {
        let network = affinities(
            &json!({"user_affinities":[{"other_user_id":"2","communication_rank":3,"communication_probability":0.9,"vc_probability":0.4},{"other_user_id":"3"}]}),
        );
        let cached = affinities(
            &json!({"userAffinities":[{"otherUserId":"2","communicationRank":3,"communicationProbability":0.9,"vcProbability":0.4},{"otherUserId":"3"}]}),
        );
        assert_eq!(network.len(), 2);
        assert_eq!(network[0].user, cached[0].user);
        assert_eq!(network[0].communication_rank, cached[0].communication_rank);
        assert_eq!(
            network[0].communication_probability,
            cached[0].communication_probability
        );
        assert_eq!(network[0].voice_probability, cached[0].voice_probability);
        assert_eq!(network[1].communication_rank, u64::MAX);
        assert_eq!(cached[1].communication_rank, u64::MAX);
    }

    #[test]
    fn a_friends_stream_remains_a_no_permission_card_without_a_visible_channel() {
        let mut stores = stores();
        voice(&mut stores, 2, true, true);
        stores.guilds.clear();
        let feed = feed(&stores, &[], Sort::Default, application);
        assert_eq!(feed.parties.len(), 1);
        assert!(feed.parties[0].voice.is_empty());
        assert_eq!(
            feed.parties[0].streams,
            [Stream {
                guild: 10,
                channel: 11,
                owner: 2
            }]
        );
    }
    #[test]
    fn ignored_member_hides_the_whole_shared_activity_party() {
        let mut stores = stores();
        let music = json!({"type":2,"name":"Spotify","party":{"id":"spotify:session"}});
        activity(&mut stores, 2, music.clone());
        activity(&mut stores, 3, music);
        stores.set_ignored(3, true);
        assert!(
            feed(&stores, &[], Sort::Default, application)
                .parties
                .is_empty()
        );
    }
    #[test]
    fn guild_create_replaces_its_voice_snapshot_and_retains_discoverability() {
        let mut stores = stores();
        voice(&mut stores, 2, false, true);
        let mut guild = json!({"id":"10","name":"fixture","owner_id":"1","channels":[{"id":"12","name":"voice","type":2}],
            "voice_states":[{"user_id":"3","channel_id":"12","session_id":"new","self_stream":true,"discoverable":false}]});
        stores.dispatch("GUILD_CREATE", &guild);
        assert!(stores.voice_members(10, 11).is_empty());
        assert_eq!(stores.voice_members(10, 12).len(), 1);
        assert_eq!(stores.voice_members(10, 12)[0].discoverable, Some(false));
        assert!(
            feed(&stores, &[], Sort::Default, application)
                .parties
                .is_empty()
        );
        guild["voice_states"][0]["discoverable"] = json!(true);
        stores.dispatch("GUILD_CREATE", &guild);
        let cards = feed(&stores, &[], Sort::Default, application).parties;
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].streams[0].owner, 3);
        stores.dispatch("GUILD_CREATE", &json!({"id":"10","unavailable":true}));
        assert_eq!(stores.voice_members(10, 12).len(), 1);
        guild["voice_states"] = json!([]);
        stores.dispatch("GUILD_CREATE", &guild);
        assert!(stores.voice_members(10, 12).is_empty());
        guild["voice_states"] = json!([{"user_id":"2","channel_id":"12","session_id":"initial"}]);
        stores.dispatch("INITIAL_GUILD", &guild);
        assert_eq!(stores.voice_members(10, 12)[0].user_id, 2);
    }

    #[test]
    fn partial_initial_guild_cannot_replace_cached_channels_with_an_unhydrated_patch() {
        let mut stores = stores();
        voice(&mut stores, 2, false, true);
        stores.dispatch(
            "INITIAL_GUILD",
            &json!({"id":"10", "data_mode":"partial",
                "properties":{"name":"Patched guild"},
                "partial_updates":{"channels":[]}, "voice_states":[]}),
        );
        assert!(
            stores
                .guild(10)
                .unwrap()
                .channels
                .iter()
                .any(|channel| channel.id == 11)
        );
        assert_eq!(stores.voice_members(10, 11)[0].user_id, 2);
    }

    #[test]
    fn passive_voice_updates_add_move_and_remove_people_without_losing_the_rest() {
        let mut stores = stores();
        voice(&mut stores, 2, false, true);
        stores.dispatch("PASSIVE_UPDATE_V2",&json!({"guild_id":"10","updated_channels":[],
            "updated_members":[{"user":{"id":"5","username":"New member"}}],
            "updated_voice_states":[{"user_id":"5","channel_id":"11","session_id":"new","self_stream":true,"discoverable":true}],
            "removed_voice_states":[]}));
        assert_eq!(stores.voice_members(10, 11).len(), 2);
        let cards = feed(&stores, &[], Sort::Default, application).parties;
        assert_eq!(cards[0].members, vec![2, 5]);
        assert_eq!(stores.users[&5].username, "New member");
        stores.dispatch("PASSIVE_UPDATE_V2",&json!({"guild_id":"10","updated_voice_states":[
            {"user_id":"5","channel_id":"12","session_id":"new","discoverable":false}],"removed_voice_states":["2"]}));
        assert!(stores.voice_members(10, 11).is_empty());
        assert_eq!(stores.voice_members(10, 12)[0].discoverable, Some(false));
        stores.dispatch(
            "PASSIVE_UPDATE_V2",
            &json!({"guild_id":"10","updated_channels":[]}),
        );
        assert_eq!(stores.voice_members(10, 12).len(), 1);
        stores.dispatch(
            "PASSIVE_UPDATE_V2",
            &json!({"guild_id":"10","removed_voice_states":["5"]}),
        );
        assert!(stores.voice_members(10, 12).is_empty());
    }
    #[test]
    fn gateway_string_times_keep_the_recent_game_and_its_elapsed_start() {
        let mut stores = stores();
        stores.dispatch("PRESENCE_UPDATE",&json!({"user":{"id":"2"},"status":"online","activities":[
            {"type":0,"name":"Older","application_id":"101","created_at":"1000"},
            {"type":0,"name":"Newer","application_id":"102","created_at":"2000","timestamps":{"start":"1500"}}
        ]}));
        let cards = feed(&stores, &[], Sort::Default, application).parties;
        assert_eq!(cards[0].activities[0].value["name"], "Newer");
        assert_eq!(cards[0].activities[0].started, Some(1500));
        activity(
            &mut stores,
            2,
            json!({"type":0,"name":"Fallback","application_id":"101","created_at":"2500"}),
        );
        assert_eq!(
            feed(&stores, &[], Sort::Default, application).parties[0].activities[0].started,
            Some(2500)
        );
    }
}
