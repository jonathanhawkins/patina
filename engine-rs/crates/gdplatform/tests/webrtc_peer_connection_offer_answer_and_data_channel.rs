//! pat-s3rrv.80 — WebRTC offer/answer + ICE + data channel, end to end.
//!
//! `webrtc_peer.rs` was 149 lines of mock: `create_offer` returned the literal
//! `"v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=Patina\r\n"`, `poll_connected` flipped
//! the state without reading the remote description, and delivery was a
//! `Vec<u8>` copied between two structs when a test asked for it. Nothing
//! consumed any of it.
//!
//! ⚠️ These tests drive the **signalling loop**, not just the state machine:
//! every description and every ICE candidate is relayed the way an application's
//! signalling channel would relay it, and the connection refuses to open until
//! all three parts have arrived. [`ice_relay_is_load_bearing`] is the control
//! that proves it — drop the candidates and the handshake does not complete. A
//! test that only asserted the happy path would pass against a state machine
//! that ignored its inputs entirely, which is exactly what the mock did.

use gdplatform::network::{ConnectionStatus, NetworkPeer, Packet, PeerId, TransferMode};
use gdplatform::webrtc_peer::{
    ChannelState, ConnectionState, DataChannelConfig, WebRTCDataChannel, WebRTCMultiplayerPeer,
    WebRTCPeerConnection, WriteMode,
};

/// Relays every pending description and candidate from `from` to `to`, as an
/// application's signalling channel does.
fn signal(from: &mut WebRTCPeerConnection, to: &mut WebRTCPeerConnection) {
    while let Some(desc) = from.next_session_description() {
        // The generating side applies its own description first, exactly as a
        // Godot script does in the `session_description_created` handler.
        from.set_local_description(&desc.kind, &desc.sdp)
            .expect("local description applies");
        to.set_remote_description(&desc.kind, &desc.sdp)
            .expect("remote description applies");
    }
    while let Some(candidate) = from.next_ice_candidate() {
        to.add_ice_candidate(&candidate.media, candidate.index, &candidate.name)
            .expect("candidate applies");
    }
}

/// Runs the full exchange and hands back two connected peers and their channels.
fn connected_pair() -> (
    WebRTCPeerConnection,
    WebRTCPeerConnection,
    WebRTCDataChannel,
    WebRTCDataChannel,
) {
    let mut a = WebRTCPeerConnection::new();
    let mut b = WebRTCPeerConnection::new();

    // ⚠️ Channels are opened BEFORE negotiation: Godot refuses `add_peer` on a
    // connection that has left STATE_NEW, because a channel added afterwards
    // would not be in the SDP the remote already holds.
    let channel_a = a
        .create_data_channel("game", DataChannelConfig::default())
        .expect("channel opens on a new connection");
    let channel_b = b
        .create_data_channel("game", DataChannelConfig::default())
        .expect("channel opens on a new connection");

    a.create_offer().expect("offer generates");
    signal(&mut a, &mut b); // offer + a's candidates
    signal(&mut b, &mut a); // the answer b generated, + b's candidates

    a.poll().expect("poll");
    b.poll().expect("poll");
    (a, b, channel_a, channel_b)
}

/// **The acceptance test.**
#[test]
fn webrtc_peer_connection_offer_answer_and_data_channel() {
    let (a, b, mut channel_a, mut channel_b) = connected_pair();

    assert_eq!(a.get_connection_state(), ConnectionState::Connected);
    assert_eq!(b.get_connection_state(), ConnectionState::Connected);
    assert_eq!(channel_a.get_ready_state(), ChannelState::Open);
    assert_eq!(channel_b.get_ready_state(), ChannelState::Open);

    channel_a.put_packet(b"ping".to_vec()).expect("A sends");
    assert_eq!(
        channel_b.get_available_packet_count(),
        1,
        "the packet must arrive without a test poking a delivery helper — the \
         mock this replaced only moved bytes when asked to",
    );
    assert_eq!(channel_b.get_packet(), Some(b"ping".to_vec()));

    // And the reverse direction, so the binding is not one-way.
    channel_b.put_packet(b"pong".to_vec()).expect("B sends");
    assert_eq!(channel_a.get_packet(), Some(b"pong".to_vec()));
    assert_eq!(channel_a.get_packet(), None);
}

/// 🔑 The SDP is **generated**, not the canned literal the mock returned.
#[test]
fn the_offer_carries_this_connections_own_credentials() {
    let mut a = WebRTCPeerConnection::new();
    a.create_offer().expect("offer generates");
    let offer = a.next_session_description().expect("an offer was queued");

    assert_eq!(offer.kind, "offer");
    assert!(
        !offer.sdp.contains("s=Patina"),
        "the hardcoded SDP literal must be gone; got {}",
        offer.sdp,
    );
    assert!(
        offer
            .sdp
            .contains(&format!("a=ice-ufrag:{}", a.ice_ufrag())),
        "the offer must advertise this connection's own ICE ufrag, or no remote \
         can resolve it; got {}",
        offer.sdp,
    );
    assert!(
        offer
            .sdp
            .contains("m=application 9 UDP/DTLS/SCTP webrtc-datachannel"),
        "a datachannel session needs its media line; got {}",
        offer.sdp,
    );
}

/// 🔑 The control: without the ICE relay the handshake does not complete.
///
/// ⚠️ This is what separates a real state machine from the mock's
/// `poll_connected`, which flipped to Connected regardless of what it had been
/// given. If this test ever passes, the descriptions and candidates have stopped
/// being load-bearing and every other test here is proving nothing.
#[test]
fn ice_relay_is_load_bearing() {
    let mut a = WebRTCPeerConnection::new();
    let mut b = WebRTCPeerConnection::new();

    a.create_offer().expect("offer generates");
    // Descriptions only — every candidate is dropped on the floor.
    while let Some(desc) = a.next_session_description() {
        a.set_local_description(&desc.kind, &desc.sdp)
            .expect("local");
        b.set_remote_description(&desc.kind, &desc.sdp)
            .expect("remote");
    }
    while let Some(desc) = b.next_session_description() {
        b.set_local_description(&desc.kind, &desc.sdp)
            .expect("local");
        a.set_remote_description(&desc.kind, &desc.sdp)
            .expect("remote");
    }

    a.poll().expect("poll");
    b.poll().expect("poll");

    assert_ne!(
        a.get_connection_state(),
        ConnectionState::Connected,
        "a connection with no remote ICE candidate must not report Connected",
    );
}

/// A description naming a peer that does not exist fails loudly.
#[test]
fn an_unknown_remote_fails_rather_than_hanging() {
    let mut a = WebRTCPeerConnection::new();
    let result = a.set_remote_description("offer", "v=0\r\na=ice-ufrag:ghost\r\n");

    assert!(result.is_err(), "an unresolvable ufrag must be an error");
    assert_eq!(
        a.get_connection_state(),
        ConnectionState::Failed,
        "reporting Connecting forever would read as a slow network rather than \
         a peer that was never there",
    );
}

/// Text mode is carried with the packet — Godot's `was_string_packet`.
#[test]
fn write_mode_travels_with_the_packet() {
    let (_a, _b, mut channel_a, mut channel_b) = connected_pair();

    channel_a.set_write_mode(WriteMode::Text);
    channel_a.put_packet(b"hello".to_vec()).expect("A sends");
    assert_eq!(channel_b.get_packet(), Some(b"hello".to_vec()));
    assert!(
        channel_b.was_string_packet(),
        "a packet written in text mode must read back as a string packet",
    );

    channel_a.set_write_mode(WriteMode::Binary);
    channel_a.put_packet(vec![0xff, 0x00]).expect("A sends");
    assert_eq!(channel_b.get_packet(), Some(vec![0xff, 0x00]));
    assert!(
        !channel_b.was_string_packet(),
        "the flag must follow each packet, not latch on the first one",
    );
}

/// 🔑 `create_mesh(1)` + `add_peer` builds a peer the MultiplayerAPI accepts.
#[test]
fn a_mesh_peer_can_be_handed_to_the_multiplayer_api() {
    let mut mesh = WebRTCMultiplayerPeer::new();
    mesh.create_mesh(1).expect("mesh initializes");
    mesh.add_peer(WebRTCPeerConnection::new(), 2)
        .expect("a fresh connection is addable");

    assert_eq!(mesh.get_unique_id(), PeerId(1));
    assert!(mesh.is_server(), "id 1 is the authority");
    assert!(mesh.has_peer(2));
    assert_eq!(mesh.get_connection_status(), ConnectionStatus::Connected);

    // The point of the exercise: it is a NetworkPeer, so it can be the
    // transport. The mock never implemented the trait, so it could not be.
    let mut api = gdplatform::network::MultiplayerAPI::new();
    api.set_multiplayer_peer(Box::new(mesh));
}

/// Godot refuses a client with id 1 — two authorities is worse than an error.
#[test]
fn a_client_may_not_claim_the_server_id() {
    let mut peer = WebRTCMultiplayerPeer::new();
    assert!(
        peer.create_client(1).is_err(),
        "id 1 belongs to the server (webrtc_multiplayer_peer.cpp:195)",
    );
    assert!(peer.create_client(2).is_ok());
}

/// A peer added after negotiation would not be in the SDP the remote holds.
#[test]
fn add_peer_refuses_an_already_negotiated_connection() {
    let (a, _b, _ca, _cb) = connected_pair();

    let mut mesh = WebRTCMultiplayerPeer::new();
    mesh.create_mesh(1).expect("mesh initializes");
    assert!(
        mesh.add_peer(a, 2).is_err(),
        "add_peer needs STATE_NEW (webrtc_multiplayer_peer.cpp:297)",
    );
}

/// Relays one signalling pass between two multiplayer peers' connections.
///
/// Descriptions are applied locally as they are drained — that is what triggers
/// gathering — so the candidates picked up afterwards in the same pass are the
/// ones that description produced.
fn signal_between(
    from: &mut WebRTCMultiplayerPeer,
    from_peer: u32,
    to: &mut WebRTCMultiplayerPeer,
    to_peer: u32,
) {
    let mut descriptions = Vec::new();
    let mut candidates = Vec::new();
    if let Some(conn) = from.peer_connection_mut(from_peer) {
        while let Some(d) = conn.next_session_description() {
            conn.set_local_description(&d.kind, &d.sdp)
                .expect("local description applies");
            descriptions.push(d);
        }
        while let Some(c) = conn.next_ice_candidate() {
            candidates.push(c);
        }
    }
    if let Some(conn) = to.peer_connection_mut(to_peer) {
        for d in &descriptions {
            conn.set_remote_description(&d.kind, &d.sdp)
                .expect("remote description applies");
        }
        for c in &candidates {
            conn.add_ice_candidate(&c.media, c.index, &c.name)
                .expect("candidate applies");
        }
    }
}

/// Packets routed through the multiplayer peer reach the right channel.
#[test]
fn the_multiplayer_peer_routes_packets_over_its_reserved_channels() {
    let mut one = WebRTCMultiplayerPeer::new();
    let mut two = WebRTCMultiplayerPeer::new();
    one.create_mesh(1).expect("mesh");
    two.create_mesh(2).expect("mesh");

    // Added while still NEW, which is also what opens the reserved channels.
    one.add_peer(WebRTCPeerConnection::new(), 2).expect("added");
    two.add_peer(WebRTCPeerConnection::new(), 1).expect("added");

    one.peer_connection_mut(2)
        .expect("peer 2's connection")
        .create_offer()
        .expect("offer generates");
    signal_between(&mut one, 2, &mut two, 1); // offer + one's candidates
    signal_between(&mut two, 1, &mut one, 2); // answer + two's candidates

    let _ = one.poll();
    let _ = two.poll();

    assert!(
        one.peer_connection_mut(2)
            .expect("peer 2's connection")
            .next_received_channel()
            .is_none(),
        "the reserved multiplayer channels are negotiated:true — both sides \
         create them, so Godot fires no data_channel_received for them",
    );

    one.send_packet(
        PeerId(2),
        Packet {
            sender: PeerId(1),
            channel: 0,
            data: b"hi".to_vec(),
            transfer_mode: TransferMode::Reliable,
        },
    )
    .expect("send");

    let received = two.poll();
    assert_eq!(
        received.len(),
        1,
        "the packet must cross the data channel the multiplayer peer opened",
    );
    assert_eq!(received[0].data, b"hi".to_vec());
    assert_eq!(
        received[0].sender,
        PeerId(1),
        "the sender is the peer id the receiver filed the connection under",
    );
}

/// 🔑 `data_channel_received` reports the channel the REMOTE opened.
///
/// ⚠️ The event is easy to fake by echoing our own channels back, which would
/// fire it for channels we created ourselves and never fire it for the remote's.
/// Both peers here open a channel with the same label, so the only way to tell
/// the two implementations apart is that each side sees exactly ONE event.
#[test]
fn a_non_negotiated_channel_is_announced_to_the_remote() {
    let (mut a, mut b, _channel_a, _channel_b) = connected_pair();

    assert_eq!(
        a.next_received_channel().as_deref(),
        Some("game"),
        "A must learn about the channel B opened",
    );
    assert_eq!(
        b.next_received_channel().as_deref(),
        Some("game"),
        "and B about A's",
    );
    assert_eq!(
        a.next_received_channel(),
        None,
        "one remote channel means one event — a second would mean our own \
         channel was echoed back as if the remote had opened it",
    );
}

/// Two channels with one label would alias into a single mailbox queue.
#[test]
fn a_duplicate_channel_label_is_refused() {
    let mut a = WebRTCPeerConnection::new();
    a.create_data_channel("game", DataChannelConfig::default())
        .expect("the first channel opens");
    assert!(
        a.create_data_channel("game", DataChannelConfig::default())
            .is_err(),
        "a second 'game' channel would share the first one's queue, which reads \
         as packet loss on whichever channel drains second",
    );
    assert!(
        a.create_data_channel("chat", DataChannelConfig::default())
            .is_ok(),
        "a distinct label is still fine",
    );
}
