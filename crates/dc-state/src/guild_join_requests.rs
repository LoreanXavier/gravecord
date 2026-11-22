//! The two join-request stores of the official client.
//!
//! `UserGuildJoinRequestStore` (module 212455): this account's own request to join each server, kept by guild.
//! `GuildJoinRequestStoreV2` (module 844944): every request the moderators of a server look at, indexed by status.
//!
//! The gateway sends `GUILD_JOIN_REQUEST_CREATE` and `GUILD_JOIN_REQUEST_UPDATE {request, status, guild_id}` and
//! `GUILD_JOIN_REQUEST_DELETE {id, user_id, guild_id}` (module 225123). The user store keeps only the account's own
//! request and forgets it once it is approved and seen (`NK`, module 821124); the moderators' store ignores the
//! account's own request (`R`). `UserStore` caches the user and the moderator of a request (module 287809 `eL`).

use crate::{Id, Stores, User, id, text};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// `B5` (module 513461).
pub const SUBMITTED: &str = "SUBMITTED";
pub const REJECTED: &str = "REJECTED";
pub const APPROVED: &str = "APPROVED";

/// How long a fetched list stays fresh (`v`, module 844944: ten minutes).
const FRESH_FOR: Duration = Duration::from_secs(10 * 60);

/// A join request as the stores keep it (`_`, module 212455).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JoinRequest {
    pub join_request_id: String,
    pub guild_id: Option<Id>,
    pub user_id: Option<Id>,
    pub user: Value,
    pub created_at: Option<String>,
    pub form_responses: Value,
    pub rejection_reason: Option<String>,
    pub application_status: Option<String>,
    pub actioned_at: Option<String>,
    pub actioned_by_user: Value,
    pub last_seen: Option<String>,
    pub interview_channel_id: Option<Id>,
}

impl JoinRequest {
    pub fn from_value(raw: &Value) -> JoinRequest {
        JoinRequest {
            join_request_id: text(&raw["join_request_id"]).unwrap_or_default(),
            guild_id: id(&raw["guild_id"]),
            user_id: id(&raw["user_id"]),
            user: raw["user"].clone(),
            created_at: text(&raw["created_at"]),
            form_responses: raw["form_responses"].clone(),
            rejection_reason: text(&raw["rejection_reason"]),
            application_status: text(&raw["application_status"]),
            actioned_at: text(&raw["actioned_at"]),
            actioned_by_user: raw["actioned_by_user"].clone(),
            last_seen: text(&raw["last_seen"]),
            interview_channel_id: id(&raw["interview_channel_id"]),
        }
    }

    /// `NK`: approved and already seen, so the account's own notice is done.
    fn approved_and_seen(&self) -> bool {
        self.application_status.as_deref() == Some(APPROVED) && self.last_seen.is_some()
    }
}

#[derive(Default)]
pub struct JoinRequests {
    /// `UserGuildJoinRequestStore`'s `d`: guild to this account's request.
    own: HashMap<Id, JoinRequest>,
    /// Its coach mark guild (`o`).
    coachmark: Option<Id>,
    /// `GuildJoinRequestStoreV2`'s `b`: request id to request.
    by_id: HashMap<String, JoinRequest>,
    /// Per guild, how many requests are submitted (`A`), with the time it was last set (`E`).
    submitted_totals: HashMap<Id, (u64, Instant)>,
    pub version: u64,
}

impl JoinRequests {
    /// `UserGuildJoinRequestStore.getRequest`.
    pub fn get_request(&self, guild: Id) -> Option<&JoinRequest> {
        self.own.get(&guild)
    }

    /// `computeGuildIds`.
    pub fn compute_guild_ids(&self) -> Vec<Id> {
        let mut guilds: Vec<Id> = self
            .own
            .values()
            .filter_map(|request| request.guild_id)
            .collect();
        guilds.sort_unstable();
        guilds
    }

    /// `hasJoinRequestCoackmark`.
    pub fn has_join_request_coachmark(&self) -> bool {
        self.coachmark.is_some()
    }

    pub fn show_coachmark(&mut self, guild: Id) {
        self.coachmark = Some(guild);
    }

    pub fn clear_coachmark(&mut self) {
        self.coachmark = None;
    }

    /// `GuildJoinRequestStoreV2.getRequest`.
    pub fn get_request_by_id(&self, request: &str) -> Option<&JoinRequest> {
        self.by_id.get(request)
    }

    /// Look up a submitted join request by its interview channel id (module 561446 / 844944).
    pub fn get_request_by_interview_channel(&self, channel_id: Id) -> Option<&JoinRequest> {
        self.by_id
            .values()
            .find(|req| req.interview_channel_id == Some(channel_id))
    }

    /// `getRequests(guild, status)`: the actioned lists sort by `actionedAt` and the others by request id
    /// (`g`, `S`, `N`, module 844944; module 713402 keeps each sorted ascending by that key).
    pub fn get_requests(&self, guild: Id, status: &str) -> Vec<&JoinRequest> {
        let mut found: Vec<&JoinRequest> = self
            .by_id
            .values()
            .filter(|request| request.guild_id == Some(guild))
            .filter(|request| match status {
                APPROVED | REJECTED => request.application_status.as_deref() == Some(status),
                SUBMITTED => request.application_status.as_deref() == Some(SUBMITTED),
                other => request.application_status.as_deref() == Some(other),
            })
            .collect();
        if status == APPROVED || status == REJECTED {
            found.sort_by(|a, b| a.actioned_at.cmp(&b.actioned_at));
        } else {
            found.sort_by(|a, b| a.join_request_id.cmp(&b.join_request_id));
        }
        found
    }

    /// `getSubmittedGuildJoinRequestTotal`.
    pub fn get_submitted_total(&self, guild: Id) -> Option<u64> {
        self.submitted_totals.get(&guild).map(|(total, _)| *total)
    }

    /// `hasFetched`: the total was set less than ten minutes ago.
    pub fn has_fetched(&self, guild: Id) -> bool {
        self.submitted_totals
            .get(&guild)
            .is_some_and(|(_, at)| at.elapsed() < FRESH_FOR)
    }

    /// `GUILD_JOIN_REQUESTS_FETCH_SUCCESS`: the submitted total is known and every request is stored.
    pub fn fetched(&mut self, guild: Id, status: &str, total: u64, requests: &[Value]) {
        if status == SUBMITTED {
            self.submitted_totals.insert(guild, (total, Instant::now()));
        }
        for request in requests {
            self.put(JoinRequest::from_value(request));
        }
        self.version += 1;
    }

    fn forget_own(&mut self, guild: Id) {
        self.own.remove(&guild);
        if self.coachmark == Some(guild) {
            self.coachmark = None;
        }
    }

    /// `O`: the request enters the list its status belongs to.
    fn put(&mut self, request: JoinRequest) {
        self.by_id.insert(request.join_request_id.clone(), request);
    }

    /// `h`: a request moved between statuses changes the submitted total by one.
    fn recount(&mut self, guild: Id, new: Option<&str>, old: Option<&str>) {
        if new == old || new.is_none() {
            return;
        }
        // A server whose total was never fetched has nothing to adjust.
        let Some((mut total, _)) = self.submitted_totals.get(&guild).copied() else {
            return;
        };
        if new == Some(SUBMITTED) {
            total += 1;
        }
        if old == Some(SUBMITTED) {
            total = total.saturating_sub(1);
        }
        self.submitted_totals.insert(guild, (total, Instant::now()));
    }
}

impl Stores {
    pub(crate) fn join_requests_dispatch(&mut self, name: &str, data: &Value) {
        match name {
            "READY" => {
                let store = &mut self.join_requests;
                store.own.clear();
                for raw in data["guild_join_requests"].as_array().into_iter().flatten() {
                    if let Some(guild) = id(&raw["guild_id"]) {
                        store.own.insert(guild, JoinRequest::from_value(raw));
                    }
                }
                store.version += 1;
            }
            "GUILD_JOIN_REQUEST_CREATE" | "GUILD_JOIN_REQUEST_UPDATE" => {
                let Some(guild) = id(&data["guild_id"]) else {
                    return;
                };
                if !data["request"].is_object() {
                    return;
                }
                let request = JoinRequest::from_value(&data["request"]);
                for key in ["user", "actioned_by_user"] {
                    if let Some(user) = User::from_value(&data["request"][key]) {
                        self.users.insert(user.id, user);
                    }
                }
                let me = self.current_user.as_ref().map(|user| user.id);
                let store = &mut self.join_requests;
                // UserGuildJoinRequestStore `A`: only the account's own request, dropped once approved and seen.
                if me.is_none() || request.user_id == me {
                    if request.approved_and_seen() {
                        store.forget_own(guild);
                    } else {
                        store.own.insert(guild, request.clone());
                    }
                }
                // GuildJoinRequestStoreV2 `R`: every request but the account's own.
                if me.is_some() && request.user_id != me {
                    let old = store
                        .by_id
                        .get(&request.join_request_id)
                        .and_then(|known| known.application_status.clone());
                    store.recount(guild, request.application_status.as_deref(), old.as_deref());
                    store.put(request);
                }
                store.version += 1;
            }
            "GUILD_JOIN_REQUEST_DELETE" => {
                let Some(guild) = id(&data["guild_id"]) else {
                    return;
                };
                let store = &mut self.join_requests;
                store.forget_own(guild);
                if let Some(request) = text(&data["id"])
                    && let Some(known) = store.by_id.remove(&request)
                {
                    store.recount(guild, Some("DELETED"), known.application_status.as_deref());
                }
                store.version += 1;
            }
            "GUILD_DELETE" => {
                if let Some(guild) = id(&data["id"]) {
                    self.join_requests.forget_own(guild);
                }
            }
            // Module 624458's `removeGuildJoinRequest` (`request: null`) and `resetGuildJoinRequest` (the answer's
            // request) dispatch this after their request succeeds.
            "USER_GUILD_JOIN_REQUEST_UPDATE" => {
                let Some(guild) = id(&data["guildId"]) else {
                    return;
                };
                let store = &mut self.join_requests;
                if data["request"].is_object() {
                    store
                        .own
                        .insert(guild, JoinRequest::from_value(&data["request"]));
                } else {
                    store.forget_own(guild);
                }
                store.version += 1;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(id: &str, user: &str, status: &str) -> Value {
        json!({
            "join_request_id": id, "guild_id": "5", "user_id": user,
            "user": {"id": user, "username": "applicant", "discriminator": "0"},
            "created_at": "2026-11-11T10:00:00.000000+00:00",
            "application_status": status, "form_responses": [], "last_seen": null,
        })
    }

    fn event(request: Value, status: &str) -> Value {
        json!({"request": request, "status": status, "guild_id": "5"})
    }

    fn me(stores: &mut Stores) {
        stores.current_user = Some(User {
            id: 10,
            ..User::default()
        });
    }

    #[test]
    fn the_accounts_own_request_is_kept_and_dropped_once_approved_and_seen() {
        let mut stores = Stores::default();
        me(&mut stores);
        stores.dispatch(
            "GUILD_JOIN_REQUEST_CREATE",
            &event(request("100", "10", SUBMITTED), SUBMITTED),
        );
        assert_eq!(
            stores
                .join_requests
                .get_request(5)
                .unwrap()
                .application_status
                .as_deref(),
            Some(SUBMITTED)
        );
        assert_eq!(stores.join_requests.compute_guild_ids(), vec![5]);
        // Approved but not yet seen: it stays, so the notice shows.
        stores.dispatch(
            "GUILD_JOIN_REQUEST_UPDATE",
            &event(request("100", "10", APPROVED), APPROVED),
        );
        assert!(stores.join_requests.get_request(5).is_some());
        let mut seen = request("100", "10", APPROVED);
        seen["last_seen"] = json!("2026-11-11T11:00:00.000000+00:00");
        stores.dispatch("GUILD_JOIN_REQUEST_UPDATE", &event(seen, APPROVED));
        assert!(stores.join_requests.get_request(5).is_none());
        // The account's own request never enters the moderators' lists.
        assert!(stores.join_requests.get_request_by_id("100").is_none());
    }

    #[test]
    fn another_persons_request_does_not_touch_the_accounts_own_and_fills_the_moderator_lists() {
        let mut stores = Stores::default();
        me(&mut stores);
        stores.dispatch(
            "GUILD_JOIN_REQUEST_CREATE",
            &event(request("100", "10", SUBMITTED), SUBMITTED),
        );
        stores.dispatch(
            "GUILD_JOIN_REQUEST_CREATE",
            &event(request("200", "11", SUBMITTED), SUBMITTED),
        );
        assert_eq!(
            stores.join_requests.get_request(5).unwrap().join_request_id,
            "100"
        );
        let submitted = stores.join_requests.get_requests(5, SUBMITTED);
        assert_eq!(submitted.len(), 1);
        assert_eq!(submitted[0].join_request_id, "200");
        // The applicant's user is cached.
        assert_eq!(stores.users[&11].username, "applicant");
        // Approving it moves it between the lists.
        let mut approved = request("200", "11", APPROVED);
        approved["actioned_at"] = json!("2026-11-11T12:00:00.000000+00:00");
        stores.dispatch("GUILD_JOIN_REQUEST_UPDATE", &event(approved, APPROVED));
        assert!(stores.join_requests.get_requests(5, SUBMITTED).is_empty());
        assert_eq!(stores.join_requests.get_requests(5, APPROVED).len(), 1);
    }

    #[test]
    fn the_submitted_total_follows_status_changes_and_deletes() {
        let mut stores = Stores::default();
        me(&mut stores);
        stores.join_requests.fetched(
            5,
            SUBMITTED,
            3,
            &[
                request("200", "11", SUBMITTED),
                request("201", "12", SUBMITTED),
            ],
        );
        assert_eq!(stores.join_requests.get_submitted_total(5), Some(3));
        assert!(stores.join_requests.has_fetched(5));
        stores.dispatch(
            "GUILD_JOIN_REQUEST_UPDATE",
            &event(request("200", "11", REJECTED), REJECTED),
        );
        assert_eq!(stores.join_requests.get_submitted_total(5), Some(2));
        stores.dispatch(
            "GUILD_JOIN_REQUEST_DELETE",
            &json!({"id": "201", "user_id": "12", "guild_id": "5"}),
        );
        assert_eq!(stores.join_requests.get_submitted_total(5), Some(1));
        assert!(stores.join_requests.get_request_by_id("201").is_none());
    }

    #[test]
    fn ready_loads_the_accounts_requests_and_a_delete_or_guild_delete_forgets_one() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "10", "username": "me", "discriminator": "0"},
                "guilds": [], "private_channels": [],
                "guild_join_requests": [request("100", "10", SUBMITTED)],
            }),
        );
        assert!(stores.join_requests.get_request(5).is_some());
        stores.join_requests.show_coachmark(5);
        assert!(stores.join_requests.has_join_request_coachmark());
        stores.dispatch(
            "GUILD_JOIN_REQUEST_DELETE",
            &json!({"id": "100", "user_id": "10", "guild_id": "5"}),
        );
        assert!(stores.join_requests.get_request(5).is_none());
        assert!(!stores.join_requests.has_join_request_coachmark());
        stores.dispatch(
            "GUILD_JOIN_REQUEST_CREATE",
            &event(request("101", "10", SUBMITTED), SUBMITTED),
        );
        stores.dispatch("GUILD_DELETE", &json!({"id": "5"}));
        assert!(stores.join_requests.get_request(5).is_none());
    }

    #[test]
    fn withdraw_and_reapply_replace_the_accounts_own_request() {
        let mut stores = Stores::default();
        me(&mut stores);
        stores.dispatch(
            "USER_GUILD_JOIN_REQUEST_UPDATE",
            &json!({"guildId": "5", "request": request("100", "10", REJECTED)}),
        );
        assert_eq!(
            stores
                .join_requests
                .get_request(5)
                .and_then(|known| known.application_status.as_deref()),
            Some(REJECTED)
        );
        stores.dispatch(
            "USER_GUILD_JOIN_REQUEST_UPDATE",
            &json!({"guildId": "5", "request": null}),
        );
        assert!(stores.join_requests.get_request(5).is_none());
    }
}
