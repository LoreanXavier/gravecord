//! The voice server's DAVE role: the MLS external sender and delivery service of one voice room.
//!
//! What the server does here is read from the client's side of the protocol (`crates/dc-voice/src/dave.rs`, `davey`,
//! the op table in `design/snapshot/dumps/VOICE.md`) and from Serein's test delivery service, which
//! `crates/dc-voice/src/video/loopback.rs` carries (`Delivery`, MIT OR Apache-2.0):
//!
//! * op 25 hands each connection the external sender (the server's signing key) once, first;
//! * the first member to send a key package (op 26) creates the group: its own pending group at epoch 0 is the group;
//! * every later key package becomes an external Add proposal (op 27, `[0] + VLBytes(proposal)`) sent to every member
//!   already in the group (each must hold the proposal a commit refers to); a member leaving becomes an external
//!   Remove proposal the same way; the first commit that comes back is taken, later ones are dropped;
//! * the taken op 28 is split into commit and welcome; the commit goes to every member as op 29, the committer
//!   included (it merges its own pending commit), the welcome to the new member as op 30, both under a fresh
//!   transition id; once every member answered op 23 (or a second passed) op 22 executes the transition.
//!
//! What is ours, not Discord's: one change per commit, first commit wins, and the one-second bound. The leaf layout follows RFC 9420 (an Add fills the leftmost blank leaf, a Remove blanks it and
//! trailing blanks are truncated), which is what the committing client computes.
use openmls::prelude::tls_codec::Serialize as _;
use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use std::collections::{HashMap, HashSet, VecDeque};

/// A connection in a room, as the delivery service knows it.
pub(crate) type Member = u64;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Send {
    /// A binary frame: op and payload (the server adds its sequence number).
    Binary(Member, u8, Vec<u8>),
    /// A JSON op: op number and data.
    Text(Member, u8, serde_json::Value),
}

enum Change {
    Add(Member, Box<KeyPackage>),
    Remove(Member),
}

struct Transition {
    id: u16,
    waiting: HashSet<Member>,
    started: std::time::Instant,
}

pub(crate) struct Group {
    signer: SignatureKeyPair,
    external: Vec<u8>,
    group_id: Vec<u8>,
    epoch: u64,
    /// Leaf index to member; `None` is a blank leaf.
    leaves: Vec<Option<Member>>,
    queue: VecDeque<Change>,
    /// The change proposed to the members (with the one it went to first), waiting for a commit.
    proposed: Option<(Member, Change)>,
    transition: Option<Transition>,
    /// Members whose leaf is still in the group but who reset their own session: they wait for their removal, and get
    /// no commit until they are added again.
    detached: HashSet<Member>,
    next_transition: u16,
    pub commits: u64,
    pub welcomes: u64,
}

impl Group {
    /// The group of a room whose DAVE group id is `channel` (big-endian bytes, as `davey` makes it).
    pub(crate) fn new(channel: u64) -> Self {
        let signer = SignatureKeyPair::new(SignatureScheme::ECDSA_SECP256R1_SHA256)
            .expect("P-256 signing keys can be made");
        let external = ExternalSender::new(
            signer.public().to_vec().into(),
            BasicCredential::new(b"gravecord call simulator".to_vec()).into(),
        )
        .tls_serialize_detached()
        .expect("an external sender serializes");
        Self {
            signer,
            external,
            group_id: channel.to_be_bytes().to_vec(),
            epoch: 0,
            leaves: Vec::new(),
            queue: VecDeque::new(),
            proposed: None,
            transition: None,
            detached: HashSet::new(),
            next_transition: 1,
            commits: 0,
            welcomes: 0,
        }
    }

    /// Op 25, the first thing a connection gets after its session description.
    pub(crate) fn external_sender(&self) -> Vec<u8> {
        self.external.clone()
    }

    /// Members in the group that take part in its transitions.
    pub(crate) fn members(&self) -> Vec<Member> {
        self.leaves
            .iter()
            .flatten()
            .copied()
            .filter(|member| !self.detached.contains(member))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }

    fn leaf_of(&self, member: Member) -> Option<usize> {
        self.leaves.iter().position(|leaf| *leaf == Some(member))
    }

    /// Op 26 from `member`.
    pub(crate) fn key_package(
        &mut self,
        member: Member,
        payload: &[u8],
    ) -> Result<Vec<Send>, &'static str> {
        let package = KeyPackageIn::tls_deserialize_exact_bytes(payload)
            .map_err(|_| "malformed key package")?
            .validate(
                OpenMlsRustCrypto::default().crypto(),
                ProtocolVersion::Mls10,
            )
            .map_err(|_| "invalid key package")?;
        // A member that sends a new key package (a reset, a prepared epoch) is out of the group from now on: the others
        // remove its old leaf before it is added again.
        self.queue
            .retain(|change| !matches!(change, Change::Add(m, _) if *m == member));
        if self.leaf_of(member).is_some() && !self.detached.contains(&member) {
            if self.members().len() == 1 {
                self.leaves.clear();
            } else {
                self.detached.insert(member);
                self.queue.push_back(Change::Remove(member));
            }
        }
        if self.members().is_empty() && self.proposed.is_none() {
            // The first member's own pending group, at epoch 0, is the group.
            self.leaves = vec![Some(member)];
            self.epoch = 0;
            return Ok(Vec::new());
        }
        self.queue.push_back(Change::Add(member, Box::new(package)));
        Ok(self.next())
    }

    /// `member` left the room; `room` is how many connections remain in it.
    pub(crate) fn left(&mut self, member: Member, room: usize) -> Vec<Send> {
        self.queue
            .retain(|change| !matches!(change, Change::Add(m, _) if *m == member));
        if let Some(transition) = self.transition.as_mut() {
            transition.waiting.remove(&member);
        }
        if self
            .proposed
            .as_ref()
            .is_some_and(|(_, change)| matches!(change, Change::Add(m, _) if *m == member))
        {
            // The commit will never come (or adds someone who is gone): start over from what remains.
            self.proposed = None;
        }
        if self.leaf_of(member).is_some() {
            let others: Vec<Member> = self
                .members()
                .into_iter()
                .filter(|m| *m != member)
                .collect();
            if room == 0 || others.is_empty() {
                // No members remain in the room: reset the group back to epoch 0.
                self.leaves = others.into_iter().map(Some).collect();
                self.epoch = 0;
                self.detached.clear();
                self.queue
                    .retain(|change| matches!(change, Change::Add(..)));
                self.proposed = None;
                self.transition = None;
            } else if self.detached.insert(member) {
                // It gets nothing more; the others commit its removal.
                self.queue.push_back(Change::Remove(member));
            }
        }
        let mut out = self.next();
        out.extend(self.try_execute(false));
        out
    }

    fn truncate(&mut self) {
        while self.leaves.last().is_some_and(Option::is_none) {
            self.leaves.pop();
        }
    }

    /// The next queued change goes to the committer, if nothing is in flight.
    fn next(&mut self) -> Vec<Send> {
        if self.proposed.is_some() || self.transition.is_some() {
            return Vec::new();
        }
        if self.members().is_empty() {
            // Nobody holds the group any more: the first one waiting creates it, from its own pending group.
            self.leaves.clear();
            self.detached.clear();
            self.epoch = 0;
            self.queue
                .retain(|change| matches!(change, Change::Add(..)));
            if let Some(Change::Add(first, _)) = self.queue.pop_front() {
                self.leaves.push(Some(first));
            }
        }
        let Some(committer) = self.members().first().copied() else {
            return Vec::new();
        };
        while let Some(change) = self.queue.pop_front() {
            let group_id = GroupId::from_slice(&self.group_id);
            let epoch = GroupEpoch::from(self.epoch);
            let message = match &change {
                Change::Add(_, package) => ExternalProposal::new_add::<OpenMlsRustCrypto>(
                    (**package).clone(),
                    group_id,
                    epoch,
                    &self.signer,
                    SenderExtensionIndex::new(0),
                )
                .ok(),
                Change::Remove(member) => match self.leaf_of(*member) {
                    Some(leaf) => ExternalProposal::new_remove::<OpenMlsRustCrypto>(
                        LeafNodeIndex::new(leaf as u32),
                        group_id,
                        epoch,
                        &self.signer,
                        SenderExtensionIndex::new(0),
                    )
                    .ok(),
                    None => continue,
                },
            };
            let Some(proposal) = message.and_then(|m| m.tls_serialize_detached().ok()) else {
                continue;
            };
            let mut payload = vec![0];
            match VLBytes::new(proposal).tls_serialize_detached() {
                Ok(bytes) => payload.extend(bytes),
                Err(_) => continue,
            }
            self.proposed = Some((committer, change));
            // Every member must hold the proposal its commit refers to; the first commit to come back wins.
            return self
                .members()
                .into_iter()
                .map(|member| Send::Binary(member, 27, payload.clone()))
                .collect();
        }
        Vec::new()
    }

    /// Op 28 from `member`: its commit, and the welcome when it adds someone.
    pub(crate) fn commit(
        &mut self,
        member: Member,
        payload: &[u8],
    ) -> Result<Vec<Send>, &'static str> {
        if self.proposed.is_none() || !self.members().contains(&member) {
            // Too late: another member's commit was taken.
            return Ok(Vec::new());
        }
        let (message, welcome) =
            MlsMessageIn::tls_deserialize_bytes(payload).map_err(|_| "malformed commit")?;
        let commit = &payload[..payload.len() - welcome.len()];
        let epoch = match message.extract() {
            MlsMessageBodyIn::PublicMessage(public) => public.epoch().as_u64(),
            _ => return Err("a DAVE commit is a public message"),
        };
        let Some((_, change)) = self.proposed.as_ref() else {
            return Ok(Vec::new());
        };
        if epoch != self.epoch {
            return Err("commit for another epoch");
        }
        if matches!(change, Change::Add(..)) && welcome.is_empty() {
            return Err("an add without a welcome");
        }
        let Some((_, change)) = self.proposed.take() else {
            return Ok(Vec::new());
        };
        let id = self.next_transition;
        self.next_transition = self.next_transition.wrapping_add(1).max(1);
        let mut out = Vec::new();
        let mut framed = id.to_be_bytes().to_vec();
        framed.extend_from_slice(commit);
        let before = self.members();
        for existing in &before {
            out.push(Send::Binary(*existing, 29, framed.clone()));
        }
        match change {
            Change::Add(new, _) => {
                match self.leaves.iter().position(Option::is_none) {
                    Some(blank) => self.leaves[blank] = Some(new),
                    None => self.leaves.push(Some(new)),
                }
                let mut framed = id.to_be_bytes().to_vec();
                framed.extend_from_slice(welcome);
                out.push(Send::Binary(new, 30, framed));
                self.welcomes += 1;
            }
            Change::Remove(gone) => {
                if let Some(leaf) = self.leaf_of(gone) {
                    self.leaves[leaf] = None;
                    self.truncate();
                }
                self.detached.remove(&gone);
            }
        }
        self.epoch += 1;
        self.commits += 1;
        self.transition = Some(Transition {
            id,
            waiting: self.members().into_iter().collect(),
            started: std::time::Instant::now(),
        });
        Ok(out)
    }

    /// Op 23 from `member`.
    pub(crate) fn ready(&mut self, member: Member, id: u16) -> Vec<Send> {
        if let Some(transition) = self.transition.as_mut()
            && transition.id == id
        {
            transition.waiting.remove(&member);
        }
        self.try_execute(false)
    }

    /// Op 31 from `member`: it could not take the commit or welcome and resets (a new key package follows).
    pub(crate) fn invalid(&mut self, member: Member) -> Vec<Send> {
        if let Some(transition) = self.transition.as_mut() {
            transition.waiting.remove(&member);
        }
        // Its old leaf is removed when its new key package comes.
        self.try_execute(false)
    }

    /// Executes a transition everyone is ready for, or that waited a second; then the next change.
    pub(crate) fn tick(&mut self) -> Vec<Send> {
        let mut out = self.try_execute(true);
        if self.transition.is_none() {
            out.extend(self.next());
        }
        out
    }

    fn try_execute(&mut self, timed: bool) -> Vec<Send> {
        let Some(transition) = &self.transition else {
            return Vec::new();
        };
        let late = timed && transition.started.elapsed() >= std::time::Duration::from_secs(1);
        if !transition.waiting.is_empty() && !late {
            return Vec::new();
        }
        let id = transition.id;
        self.transition = None;
        let mut out: Vec<Send> = self
            .members()
            .into_iter()
            .map(|member| Send::Text(member, 22, serde_json::json!({"transition_id": id})))
            .collect();
        out.extend(self.next());
        out
    }

    /// For the stats line.
    pub(crate) fn describe(&self) -> String {
        format!(
            "epoch {} members {} commits {} welcomes {} queued {}{}",
            self.epoch,
            self.members().len(),
            self.commits,
            self.welcomes,
            self.queue.len(),
            if self.transition.is_some() {
                " (transition pending)"
            } else {
                ""
            }
        )
    }
}

/// Every group of one voice server, by room.
#[derive(Default)]
pub(crate) struct Groups(pub HashMap<(u64, u64), Group>);

#[cfg(test)]
mod tests {
    use super::*;

    /// A client's DAVE session, as `davey` runs it in `dc-voice`.
    struct Client {
        session: davey::DaveSession,
        id: u64,
    }

    impl Client {
        fn new(id: u64, channel: u64) -> Self {
            let pair = davey::SigningKeyPair::generate();
            Self {
                session: davey::DaveSession::new(
                    std::num::NonZeroU16::MIN,
                    id,
                    channel,
                    Some(&pair),
                )
                .unwrap(),
                id,
            }
        }
    }

    fn deliver(
        clients: &mut [Client],
        sends: Vec<Send>,
        readies: &mut Vec<(u64, u16)>,
    ) -> Vec<(u64, Vec<u8>)> {
        let mut commits = Vec::new();
        for send in sends {
            match send {
                Send::Binary(to, 27, payload) => {
                    let client = clients.iter_mut().find(|c| c.id == to).unwrap();
                    let ids: Vec<u64> = (1..=4).collect();
                    let result = client
                        .session
                        .process_proposals(
                            davey::ProposalsOperationType::APPEND,
                            &payload[1..],
                            Some(&ids),
                        )
                        .unwrap()
                        .unwrap();
                    let mut frame = result.commit;
                    frame.extend(result.welcome.unwrap_or_default());
                    commits.push((to, frame));
                }
                Send::Binary(to, 29, payload) => {
                    let client = clients.iter_mut().find(|c| c.id == to).unwrap();
                    client.session.process_commit(&payload[2..]).unwrap();
                    readies.push((to, u16::from_be_bytes([payload[0], payload[1]])));
                }
                Send::Binary(to, 30, payload) => {
                    let client = clients.iter_mut().find(|c| c.id == to).unwrap();
                    client.session.process_welcome(&payload[2..]).unwrap();
                    readies.push((to, u16::from_be_bytes([payload[0], payload[1]])));
                }
                Send::Text(_, 22, _) => {}
                other => panic!("unexpected {other:?}"),
            }
        }
        commits
    }

    #[test]
    fn three_members_join_one_leaves_and_everyone_shares_the_epoch() {
        let channel = 77;
        let mut group = Group::new(channel);
        let mut clients: Vec<Client> = (1..=3).map(|id| Client::new(id, channel)).collect();
        for client in &mut clients {
            client
                .session
                .set_external_sender(&group.external_sender())
                .unwrap();
        }
        let mut readies = Vec::new();
        for index in 0..3 {
            let id = clients[index].id;
            let package = clients[index].session.create_key_package().unwrap();
            let mut sends = group.key_package(id, &package).unwrap();
            while !sends.is_empty() {
                let commits = deliver(&mut clients, sends, &mut readies);
                sends = Vec::new();
                for (from, frame) in commits {
                    sends.extend(group.commit(from, &frame).unwrap());
                }
                for (from, id) in readies.drain(..) {
                    sends.extend(group.ready(from, id));
                }
            }
        }
        assert_eq!(group.members(), [1, 2, 3]);
        assert_eq!(group.epoch(), 2);
        for client in &clients {
            assert!(
                client.session.is_ready(),
                "member {} has the keys",
                client.id
            );
            assert_eq!(client.session.epoch().unwrap().as_u64(), 2);
        }
        // Media from 1 opens at 3.
        let sealed = clients[0]
            .session
            .encrypt_opus(&[0xf8, 0xff, 0xfe])
            .unwrap()
            .into_owned();
        assert_eq!(
            clients[2]
                .session
                .decrypt(1, davey::MediaType::AUDIO, &sealed)
                .unwrap(),
            [0xf8, 0xff, 0xfe]
        );

        // Member 2 leaves: 1 commits the removal, 3 follows.
        let gone = clients.remove(1);
        drop(gone);
        let mut sends = group.left(2, 2);
        while !sends.is_empty() {
            let commits = deliver(&mut clients, sends, &mut readies);
            sends = Vec::new();
            for (from, frame) in commits {
                sends.extend(group.commit(from, &frame).unwrap());
            }
            for (from, id) in readies.drain(..) {
                sends.extend(group.ready(from, id));
            }
        }
        assert_eq!(group.members(), [1, 3]);
        assert_eq!(group.epoch(), 3);
        assert_eq!(clients[1].session.get_user_ids().unwrap().len(), 2);
    }
}
