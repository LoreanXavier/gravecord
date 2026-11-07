//! DAVE end-to-end encryption state over `davey`'s MLS session.
//!
//! Adapted from Serein `crates/discord-voice/src/crypto.rs` (`Identity`, `Dave`),
//! MIT OR Apache-2.0. Changes: no one-to-one DM peer pinning and no identity shared with Go
//! Live streams; the validation of proposals, commits and welcomes is Serein's, unchanged.
use davey::{DaveSession, ProposalsOperationType, SigningKeyPair};
use dc_proto::voice_op::VoiceOp;
use openmls::prelude::{
    ContentType, MlsMessageBodyIn, MlsMessageIn, Proposal, ProposalOrRefType, Sender,
    SenderExtensionIndex,
    tls_codec::{DeserializeBytes, VLBytes},
};
use std::num::NonZeroU16;
use zeroize::Zeroize;

use crate::signal::MAX_SIGNAL;

/// Discord's voice participant limit, as Serein enforces it.
pub(crate) const MAX_PARTICIPANTS: usize = 64;

fn protocol_version() -> NonZeroU16 {
    NonZeroU16::MIN // DAVE protocol version 1
}

/// Ephemeral DAVE identity for one call; never persisted or formatted.
struct Identity(SigningKeyPair);
impl Drop for Identity {
    fn drop(&mut self) {
        self.0.private.zeroize();
        self.0.public.zeroize();
    }
}

pub(crate) struct Dave {
    pub session: DaveSession,
    own: u64,
    participants: Vec<u64>,
    /// Clients the voice server announced as connected.
    announced: Vec<u64>,
    pub waiting: bool,
    channel: u64,
    pub pending: Option<u16>,
    pub pending_protocol: Option<u16>,
    pub ready: bool,
    pub resets: u8,
    epochs: u16,
    identity: Identity,
    pending_commit: Option<Vec<u8>>,
}
impl Dave {
    pub fn new(own: u64, channel: u64) -> Result<Self, &'static str> {
        let identity = Identity(SigningKeyPair::generate());
        Ok(Self {
            session: DaveSession::new(protocol_version(), own, channel, Some(&identity.0))
                .map_err(|_| "DAVE initialization failed")?,
            own,
            participants: vec![own],
            announced: vec![own],
            waiting: false,
            channel,
            pending: None,
            pending_protocol: None,
            ready: false,
            resets: 0,
            epochs: 0,
            identity,
            pending_commit: None,
        })
    }
    pub fn contains(&self, user: u64) -> bool {
        self.participants.contains(&user)
    }
    /// Only this device remains announced in the call.
    pub fn alone(&self) -> bool {
        self.announced.len() == 1
    }
    /// A sole announced member with no pending transition has nobody to negotiate with.
    pub fn should_wait_for_peer(&self) -> bool {
        !self.ready
            && !self.waiting
            && !self.transitioning()
            && self.alone()
            && self.session.group().is_some()
    }
    pub fn is_group_member(&self, user: u64) -> bool {
        self.session
            .get_user_ids()
            .is_some_and(|ids| ids.contains(&user))
    }
    pub fn connect(&mut self, users: &[u64]) -> Result<bool, &'static str> {
        if users.len() > MAX_PARTICIPANTS || users.contains(&0) {
            return Err("Voice participants do not match this call");
        }
        let mut next = self.participants.clone();
        for user in users {
            if !next.contains(user) {
                if next.len() == MAX_PARTICIPANTS {
                    return Err("Voice channel exceeds the 64 participant limit");
                }
                next.push(*user);
            }
        }
        let mut announced = self.announced.clone();
        for user in users {
            if !announced.contains(user) {
                announced.push(*user);
            }
        }
        let changed = next != self.participants || announced != self.announced;
        if changed {
            self.participants = next;
            self.announced = announced;
            self.ready = false;
            self.waiting = false;
        }
        Ok(changed)
    }
    /// A departing member: the call continues.
    pub fn disconnect(&mut self, user: u64) -> Result<bool, &'static str> {
        if user == self.own {
            return Err("Discord removed this device from the call");
        }
        let before = self.participants.len() + self.announced.len();
        // The MLS group keeps them until Discord's removal commit; pruned afterwards.
        if !self.is_group_member(user) {
            self.participants.retain(|id| *id != user);
        }
        self.announced.retain(|id| *id != user);
        let changed = before != self.participants.len() + self.announced.len();
        if changed {
            self.ready = false;
            self.waiting = false;
        }
        Ok(changed)
    }
    /// Epoch zero has no media ratchets in Davey. Remain joined without sending media.
    fn wait_for_peer(&mut self) -> Result<(), &'static str> {
        self.validate_group()?;
        if !self.alone() || self.session.epoch().is_none_or(|epoch| epoch.as_u64() != 0) {
            return Err("Unexpected sole-member DAVE transition");
        }
        self.pending = None;
        self.pending_protocol = None;
        self.ready = false;
        self.waiting = true;
        Ok(())
    }
    /// Transition a sole member into waiting, reinitializing a group that still contains
    /// departed peers or has advanced beyond epoch zero.
    pub fn enter_sole_member_waiting(&mut self) -> Result<(), &'static str> {
        if !self.alone() {
            return Err("Cannot enter sole member waiting with peers announced");
        }
        if self.session.epoch().is_none_or(|epoch| epoch.as_u64() != 0)
            || self.validate_group().is_err()
        {
            self.reinitialize()?;
        }
        self.wait_for_peer()
    }
    pub fn reset(&mut self) -> Result<(), &'static str> {
        self.resets += 1;
        if self.resets > 3 {
            return Err("DAVE recovery limit reached; rejoin the call");
        }
        self.reinitialize()
    }
    pub fn reinitialize(&mut self) -> Result<(), &'static str> {
        self.transition_budget()?;
        self.ready = false;
        self.waiting = false;
        self.pending = None;
        self.pending_protocol = None;
        self.pending_commit = None;
        self.session
            .reinit(
                protocol_version(),
                self.own,
                self.channel,
                Some(&self.identity.0),
            )
            .map_err(|_| "DAVE reset failed")?;
        self.prune();
        Ok(())
    }
    /// Op 26: the op byte followed by the raw TLS KeyPackage, as libdave sends it.
    pub fn key_package(&mut self) -> Result<Vec<u8>, &'static str> {
        let package = self
            .session
            .create_key_package()
            .map_err(|_| "DAVE key package failed")?;
        Ok(dc_proto::voice_op::binary_frame(
            VoiceOp::MlsKeyPackage,
            &package,
        ))
    }
    /// Op 27 in; an op 28 commit (and welcome) frame out when we commit.
    pub fn proposals(&mut self, payload: &[u8]) -> Result<Option<Vec<u8>>, &'static str> {
        if payload.len() > MAX_SIGNAL {
            return Err("DAVE proposals exceed the signaling budget");
        }
        // Initial group creation ignores proposals until the external sender has
        // established our local group.
        if self.session.group().is_none() {
            return Ok(None);
        }
        let (&operation, data) = payload.split_first().ok_or("Truncated DAVE proposal")?;
        let operation = match operation {
            0 => ProposalsOperationType::APPEND,
            1 => ProposalsOperationType::REVOKE,
            _ => return Err("Unsupported DAVE proposal operation"),
        };
        if operation == ProposalsOperationType::APPEND {
            let wire: VLBytes = VLBytes::tls_deserialize_exact_bytes(data)
                .map_err(|_| "Invalid DAVE proposal vector")?;
            let mut remaining = wire.as_slice();
            let mut count = 0;
            while !remaining.is_empty() {
                count += 1;
                if count > MAX_PARTICIPANTS * 2 {
                    return Err("Too many DAVE proposals");
                }
                let (message, rest) = MlsMessageIn::tls_deserialize_bytes(remaining)
                    .map_err(|_| "Invalid MLS proposal message")?;
                remaining = rest;
                let MlsMessageBodyIn::PublicMessage(public) = message.extract() else {
                    return Err("DAVE requires external public proposals");
                };
                if *public.sender() != Sender::External(SenderExtensionIndex::new(0))
                    || public.content_type() != ContentType::Proposal
                {
                    return Err("DAVE proposal was not sent by the external sender");
                }
            }
        }
        let result = self
            .session
            .process_proposals(operation, data, Some(&self.participants))
            .map_err(|_| "DAVE proposal validation failed")?;
        if let Some(group) = self.session.group()
            && (group.pending_proposals().count() > MAX_PARTICIPANTS * 2
                || group.pending_proposals().any(|p| {
                    !matches!(p.proposal(), Proposal::Add(_) | Proposal::Remove(_))
                        || *p.sender() != Sender::External(SenderExtensionIndex::new(0))
                        || p.proposal_or_ref_type() != ProposalOrRefType::Reference
                }))
        {
            return Err("Disallowed DAVE proposal");
        }
        match result {
            Some(result) => {
                self.pending_commit = Some(result.commit.clone());
                let mut frame = vec![VoiceOp::MlsCommitWelcome as u8];
                frame.extend(result.commit);
                if let Some(welcome) = result.welcome {
                    frame.extend(welcome);
                }
                if frame.len() > MAX_SIGNAL {
                    return Err("DAVE response exceeds call budget");
                }
                Ok(Some(frame))
            }
            None => {
                self.pending_commit = None;
                Ok(None)
            }
        }
    }
    /// Op 29 (announce commit) or 30 (welcome); returns the transition id.
    pub fn group_changed(&mut self, opcode: u8, payload: &[u8]) -> Result<u16, &'static str> {
        if payload.len() < 3 || payload.len() > MAX_SIGNAL {
            return Err("Truncated DAVE group transition");
        }
        // Davey keeps the previous epoch's decryption keys for ten seconds, so an
        // established call keeps its media flowing through a member change.
        let was_ready = self.ready;
        self.ready = false;
        self.waiting = false;
        self.transition_budget()?;
        let transition = u16::from_be_bytes([payload[0], payload[1]]);
        if opcode == VoiceOp::MlsPrepareCommitTransition as u8 {
            if self
                .session
                .epoch()
                .is_some_and(|epoch| epoch.as_u64() == 0)
                && self.pending_commit.as_deref() != Some(&payload[2..])
            {
                return Err("Initial DAVE commit differs from the locally proposed commit");
            }
            self.session
                .process_commit(&payload[2..])
                .map_err(|_| "DAVE commit validation failed")?;
        } else {
            self.session
                .process_welcome(&payload[2..])
                .map_err(|_| "DAVE welcome validation failed")?;
        }
        self.validate_group()?;
        self.pending_commit = None;
        self.prune();
        self.pending = Some(transition);
        if transition == 0 {
            self.execute(transition)?;
        } else {
            self.ready = was_ready;
        }
        Ok(transition)
    }
    /// Departed members stay authenticated only while the MLS group still contains them.
    fn prune(&mut self) {
        let members = self.session.get_user_ids().unwrap_or_default();
        let own = self.own;
        let announced = &self.announced;
        self.participants
            .retain(|id| *id == own || announced.contains(id) || members.contains(id));
    }
    fn transition_budget(&mut self) -> Result<(), &'static str> {
        self.epochs = self
            .epochs
            .checked_add(1)
            .ok_or("Call key transition budget exhausted")?;
        // Serein caps long-lived MLS storage at 1024 transitions; a rejoin starts fresh.
        if self.epochs > 1024 {
            return Err("Call key transition budget exhausted; rejoin the call");
        }
        Ok(())
    }
    fn validate_group(&self) -> Result<(), &'static str> {
        let group = self.session.group().ok_or("DAVE group is missing")?;
        let ids = self
            .session
            .get_user_ids()
            .ok_or("DAVE members are missing")?;
        if group.group_id().as_slice() != self.channel.to_be_bytes()
            || ids.is_empty()
            || ids.len() > MAX_PARTICIPANTS
            || !ids.contains(&self.own)
            || ids.iter().any(|id| !self.contains(*id))
            || ids.iter().enumerate().any(|(i, id)| ids[..i].contains(id))
        {
            return Err("DAVE group does not match the authenticated call participants");
        }
        Ok(())
    }
    /// A transition Discord has prepared but not yet executed.
    pub fn transitioning(&self) -> bool {
        self.pending.is_some() || self.pending_protocol.is_some()
    }
    /// Serein, unverified: Discord can prepare protocol version 1 before the MLS group exists.
    pub fn prepare_protocol(&mut self, id: u16) {
        self.pending_protocol = Some(id);
    }
    pub fn execute(&mut self, id: u16) -> Result<(), &'static str> {
        let is_protocol = self.pending_protocol == Some(id);
        let is_mls = self.pending == Some(id);
        if !is_protocol && !is_mls {
            return Err("Unexpected DAVE encryption transition");
        }
        if is_protocol {
            self.pending_protocol = None;
        }
        if !self.session.is_ready() {
            return if is_protocol {
                Ok(())
            } else {
                Err("Unexpected DAVE encryption transition")
            };
        }
        if !is_mls && self.pending.is_some() {
            return Ok(());
        }
        self.validate_group()?;
        self.pending = None;
        self.ready = true;
        Ok(())
    }
}

impl Drop for Dave {
    fn drop(&mut self) {
        let _ = self.session.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_package_frame_and_failed_transitions_stay_closed() {
        let mut dave = Dave::new(1, 3).unwrap();
        assert!(!dave.ready);
        assert!(dave.execute(0).is_err(), "nothing was prepared");
        // Op 26, then the TLS KeyPackage: MLS 1.0 (0x0001), DAVE ciphersuite 2 (P-256).
        let package = dave.key_package().unwrap();
        assert_eq!(&package[..5], &[26, 0, 1, 0, 2]);
        assert!(
            dave.group_changed(30, &[0, 0, 0]).is_err(),
            "garbage welcome"
        );
        assert!(!dave.ready);
        assert!(
            dave.disconnect(1).is_err(),
            "removing ourselves ends the call"
        );
    }

    #[test]
    fn roster_changes_are_bounded_and_reset_readiness() {
        let mut dave = Dave::new(1, 3).unwrap();
        assert!(dave.alone());
        assert!(dave.connect(&[2, 4]).unwrap());
        assert!(!dave.connect(&[2]).unwrap(), "already announced");
        assert!(dave.contains(4) && !dave.alone());
        assert!(dave.connect(&[0]).is_err());
        assert!(dave.disconnect(4).unwrap());
        assert!(!dave.contains(4), "not in the MLS group, so pruned at once");
        let crowd: Vec<u64> = (10..10 + MAX_PARTICIPANTS as u64).collect();
        assert!(dave.connect(&crowd).is_err(), "65 participants");
    }
}
