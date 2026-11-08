//! The whole video pipeline in one process, with nothing from the network: dc-media's software encoder, SPS
//! normalization, DAVE between two real MLS members, packetizing, pacing, the transport seal, a lossy and reordered
//! "network", the transport open, reorder with a NACK and an RTX repair, depacketizing, DAVE decryption, and the decoder
//! thread, then the picture is compared with the one the sender encoded.
//!
//! The MLS delivery service below is Serein's (`crates/discord-voice/src/test_mls.rs`, MIT OR Apache-2.0), cut down to
//! the "add one member" step; it never talks to Discord.
use super::{
    RemoteFrame, VideoHub, VideoSource,
    receive::{Decoders, Offer, Receivers},
    sender::VideoSender,
    sps,
};
use crate::{
    dave::Dave,
    rtp::{self, Mode, Transport},
};
use dc_media::{decode::Decoder, synthetic::Generator};
use openmls::prelude::tls_codec::Serialize as _;
use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};
use tokio::time::Instant;

struct Delivery {
    signer: SignatureKeyPair,
    external: Vec<u8>,
}

impl Delivery {
    fn new() -> Self {
        let signer = SignatureKeyPair::new(SignatureScheme::ECDSA_SECP256R1_SHA256).unwrap();
        let external = ExternalSender::new(
            signer.public().to_vec().into(),
            BasicCredential::new(b"local test voice server".to_vec()).into(),
        )
        .tls_serialize_detached()
        .unwrap();
        Self { signer, external }
    }

    /// The commit and the welcome that add the holder of `key_package` to `creator`'s group.
    fn add(&self, creator: &mut Dave, key_package: &[u8]) -> (Vec<u8>, Vec<u8>) {
        assert_eq!(key_package[0], 26);
        let package = KeyPackageIn::tls_deserialize_exact_bytes(&key_package[1..]).unwrap();
        let provider = OpenMlsRustCrypto::default();
        let package = package
            .validate(provider.crypto(), ProtocolVersion::Mls10)
            .unwrap();
        let group = creator.session.group().unwrap();
        let proposal = ExternalProposal::new_add::<OpenMlsRustCrypto>(
            package,
            group.group_id().clone(),
            group.epoch(),
            &self.signer,
            SenderExtensionIndex::new(0),
        )
        .unwrap()
        .tls_serialize_detached()
        .unwrap();
        let mut payload = vec![0];
        payload.extend(VLBytes::new(proposal).tls_serialize_detached().unwrap());
        let result = creator.proposals(&payload).unwrap().unwrap();
        assert_eq!(result[0], 28);
        let (_, welcome) = MlsMessageIn::tls_deserialize_bytes(&result[1..]).unwrap();
        let commit_len = result.len() - 1 - welcome.len();
        (result[1..1 + commit_len].to_vec(), welcome.to_vec())
    }
}

/// Two members of one DAVE group, both past the transition.
fn pair() -> (Dave, Dave) {
    let server = Delivery::new();
    let mut alice = Dave::new(1, 3).unwrap();
    let mut bob = Dave::new(2, 3).unwrap();
    alice.session.set_external_sender(&server.external).unwrap();
    bob.session.set_external_sender(&server.external).unwrap();
    // The roster the voice server announces (op 11) before it proposes the add.
    alice.connect(&[1, 2]).unwrap();
    bob.connect(&[1, 2]).unwrap();
    let package = bob.key_package().unwrap();
    let (commit, welcome) = server.add(&mut alice, &package);
    let id = 5u16.to_be_bytes();
    assert_eq!(
        alice
            .group_changed(29, &[&id[..], &commit].concat())
            .unwrap(),
        5
    );
    assert_eq!(
        bob.group_changed(30, &[&id[..], &welcome].concat())
            .unwrap(),
        5
    );
    alice.execute(5).unwrap();
    bob.execute(5).unwrap();
    assert!(alice.ready && bob.ready);
    (alice, bob)
}

const VIDEO_SSRC: u32 = 4000;
const RTX_SSRC: u32 = 4001;
const OWN_SSRC: u32 = 3999;

#[test]
fn a_camera_picture_survives_dave_loss_reordering_and_repair() {
    let (mut alice, mut bob) = pair();
    let key = [9u8; 32];
    let mut alice_wire = Transport::new(Mode::Aes256GcmRtpSize, &key);
    let bob_wire = Transport::new(Mode::Aes256GcmRtpSize, &key);

    // A 640x480 picture needs several packets for its keyframe, so there is something to lose.
    let mut generator = Generator::new(640, 480, 15, 900_000).unwrap();
    let frame = generator.next(false).unwrap().unwrap();
    assert!(frame.keyframe);
    let expected = sps::normalize(&frame.data).unwrap().into_owned();
    let mut sender = VideoSender::new(VIDEO_SSRC, RTX_SSRC, 900_000);
    assert!(sender.offer(&frame, &mut alice, Instant::now()).unwrap());
    assert!(
        !sender.ready(),
        "a second picture waits for the first to leave"
    );

    // The paced packets.
    let mut wire: Vec<Vec<u8>> = Vec::new();
    let started = std::time::Instant::now();
    while !sender.ready() {
        sender
            .flush(Instant::now(), &mut alice_wire, |data| {
                wire.push(data.to_vec());
                Ok(())
            })
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the pacer drains"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(wire.len() >= 4, "{} packets", wire.len());

    // The network: the third packet is lost and the next two swap places.
    let lost = wire.remove(2);
    wire.swap(2, 3);
    let lost_sequence = bob_wire.open(&lost).unwrap().sequence;

    let hub = VideoHub::new();
    let (picture_send, pictures) = mpsc::channel::<RemoteFrame>();
    hub.set_sink(Some(Arc::new(move |frame| {
        let _ = picture_send.send(frame);
    })));
    let mut decoders = Decoders::new(hub, |_| VideoSource::Camera);
    let mut receivers = Receivers::default();
    receivers.announce(1, VIDEO_SSRC, RTX_SSRC).unwrap();

    let now = Instant::now();
    for datagram in &wire {
        receivers.push(bob_wire.open(datagram).unwrap(), now);
    }
    let mut units = Vec::new();
    receivers.drain(now, &mut units);
    assert!(units.is_empty(), "the picture waits for the missing packet");

    // Bob asks; Alice resends from her history as RTX.
    let later = now + Duration::from_millis(15);
    let feedback = receivers.feedback(OWN_SSRC, later);
    let (header, body) = feedback.iter().find(|(h, _)| h[1] == 205).expect("a NACK");
    let compound = [&header[..], body].concat();
    let asked = rtp::feedback(&compound, VIDEO_SSRC).unwrap();
    assert_eq!(asked.nacks, [lost_sequence]);
    assert!(
        feedback.iter().any(|(h, _)| h[1] == 206),
        "and a PLI: bob has no keyframe yet"
    );
    assert!(
        sender.feedback(&asked, Instant::now()) || !asked.keyframe,
        "a NACK for a packet still in the history needs no new keyframe"
    );
    let mut repairs = Vec::new();
    let started = std::time::Instant::now();
    while repairs.is_empty() {
        sender
            .flush(Instant::now(), &mut alice_wire, |data| {
                repairs.push(data.to_vec());
                Ok(())
            })
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the repair leaves"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let repair = bob_wire.open(&repairs[0]).unwrap();
    assert_eq!(repair.payload_type, rtp::H264_RTX_PAYLOAD_TYPE);
    assert_eq!(repair.ssrc, RTX_SSRC);
    receivers.push(repair, later);
    receivers.drain(later, &mut units);
    assert_eq!(units.len(), 1);

    // DAVE opens it to exactly what Alice encrypted; the decoder shows it.
    let unit = units.remove(0);
    let plain = bob
        .session
        .decrypt(1, davey::MediaType::VIDEO, &unit.data)
        .unwrap();
    assert_eq!(plain, expected);
    let keyframe = dc_media::h264::is_keyframe(&plain);
    assert!(keyframe && receivers.accept(1, keyframe));
    assert_eq!(
        decoders.offer(1, plain.clone(), Instant::now()),
        Offer::Queued
    );
    let shown = pictures.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!((shown.user, shown.source), (1, VideoSource::Camera));

    let reference = Decoder::new().unwrap().decode(&expected).unwrap().unwrap();
    assert_eq!((shown.picture.width, shown.picture.height), (640, 480));
    assert_eq!(shown.picture.layout, reference.layout);
    assert_eq!(
        shown.picture.data, reference.data,
        "same pixels as a decode of the sender's picture"
    );

    // The receiver reports what it saw: one packet short, now repaired.
    let report = receivers.feedback(OWN_SSRC, later + Duration::from_secs(2));
    assert!(report.iter().any(|(h, _)| h[1] == 201), "a receiver report");
}

#[test]
fn a_third_party_without_the_group_cannot_read_the_picture() {
    let (mut alice, _bob) = pair();
    let mut stranger = Dave::new(7, 3).unwrap();
    let mut generator = Generator::new(160, 120, 15, 300_000).unwrap();
    let frame = generator.next(false).unwrap().unwrap();
    let mut sender = VideoSender::new(VIDEO_SSRC, RTX_SSRC, 300_000);
    assert!(sender.offer(&frame, &mut alice, Instant::now()).unwrap());
    let mut transport = Transport::new(Mode::XChaCha20Poly1305RtpSize, &[1; 32]);
    let mut sealed = Vec::new();
    while !sender.ready() {
        sender
            .flush(Instant::now(), &mut transport, |data| {
                sealed.push(data.to_vec());
                Ok(())
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut receivers = Receivers::default();
    receivers.announce(1, VIDEO_SSRC, RTX_SSRC).unwrap();
    let now = Instant::now();
    for datagram in &sealed {
        receivers.push(transport.open(datagram).unwrap(), now);
    }
    let mut units = Vec::new();
    receivers.drain(now, &mut units);
    assert_eq!(units.len(), 1);
    assert!(
        stranger
            .session
            .decrypt(1, davey::MediaType::VIDEO, &units[0].data)
            .is_err()
    );
}
