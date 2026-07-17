//! End-to-end WebSocketMultiplayerPeer transport tests (pat-nzgdu.50).
//!
//! Drives a real loopback `ws://` server and client through the RFC 6455
//! opening handshake to `CONNECTION_CONNECTED`, asserts the `peer_connected`
//! signal fires, exchanges packets both ways, and confirms the peer plugs into
//! `MultiplayerAPI` as a `MultiplayerPeer`.
//!
//! Both ends are polled from this single thread — the transport is poll-driven
//! and never blocks, so no background threads are needed.

use std::thread::sleep;
use std::time::Duration;

use gdplatform::multiplayer_peer_state::MultiplayerPeerSignal;
use gdplatform::network::{ConnectionStatus, MultiplayerAPI, NetworkPeer, Packet, PeerId, TransferMode};
use gdplatform::websocket_multiplayer::WebSocketMultiplayerPeer;

/// Pumps both ends until the client is fully connected (handshake + id assign).
fn pump_until_connected(server: &mut WebSocketMultiplayerPeer, client: &mut WebSocketMultiplayerPeer) {
    for _ in 0..200 {
        let _ = server.poll();
        let _ = client.poll();
        if client.get_connection_status() == ConnectionStatus::Connected
            && server.get_peer_count() == 1
        {
            return;
        }
        sleep(Duration::from_millis(5));
    }
    panic!(
        "handshake never completed: client={:?} server_peers={}",
        client.get_connection_status(),
        server.get_peer_count()
    );
}

/// Pumps both ends until `pick` returns a packet, or fails after a timeout.
fn pump_for_packet(
    a: &mut WebSocketMultiplayerPeer,
    b: &mut WebSocketMultiplayerPeer,
    expected: &[u8],
) -> Packet {
    for _ in 0..200 {
        let _ = a.poll();
        for packet in b.poll() {
            if packet.data == expected {
                return packet;
            }
        }
        sleep(Duration::from_millis(5));
    }
    panic!("packet {:?} never arrived", String::from_utf8_lossy(expected));
}

fn connected_pair() -> (WebSocketMultiplayerPeer, WebSocketMultiplayerPeer) {
    let mut server = WebSocketMultiplayerPeer::new();
    let addr = server.create_server("127.0.0.1:0").expect("server binds");

    let mut client = WebSocketMultiplayerPeer::new();
    client
        .create_client(&format!("ws://127.0.0.1:{}/", addr.port()))
        .expect("client dials");
    assert_eq!(client.get_connection_status(), ConnectionStatus::Connecting);

    pump_until_connected(&mut server, &mut client);
    (server, client)
}

#[test]
fn client_reaches_connected_and_is_assigned_a_peer_id() {
    let (server, client) = connected_pair();

    assert!(server.is_server());
    assert_eq!(server.get_unique_id(), PeerId::SERVER);

    assert!(!client.is_server());
    assert_eq!(client.get_connection_status(), ConnectionStatus::Connected);
    // The server hands out ids starting at 2; 1 is reserved for the server.
    assert_ne!(client.get_unique_id(), PeerId::SERVER);
    assert_ne!(client.get_unique_id(), PeerId(0));
}

#[test]
fn server_emits_peer_connected_signal_for_the_client() {
    let (mut server, client) = connected_pair();

    let signals = server.drain_signals();
    assert!(
        signals.contains(&MultiplayerPeerSignal::PeerConnected(
            client.get_unique_id().0 as i32
        )),
        "expected peer_connected({}), got {:?}",
        client.get_unique_id().0,
        signals
    );
    assert_eq!(server.get_peers(), vec![client.get_unique_id().0 as i32]);
    // Signals are drained, not re-emitted.
    assert!(server.drain_signals().is_empty());
}

#[test]
fn client_sees_the_server_as_a_connected_peer() {
    let (_server, mut client) = connected_pair();

    let signals = client.drain_signals();
    assert!(
        signals.contains(&MultiplayerPeerSignal::PeerConnected(PeerId::SERVER.0 as i32)),
        "client should register the server as a peer, got {:?}",
        signals
    );
}

#[test]
fn packets_flow_from_client_to_server() {
    let (mut server, mut client) = connected_pair();
    let client_id = client.get_unique_id();

    let payload = b"hello-server".to_vec();
    client
        .send_packet(
            PeerId::SERVER,
            Packet {
                sender: client_id,
                channel: 1,
                data: payload.clone(),
                transfer_mode: TransferMode::Reliable,
            },
        )
        .expect("client sends");

    let packet = pump_for_packet(&mut client, &mut server, &payload);
    assert_eq!(packet.sender, client_id);
    assert_eq!(packet.channel, 1);
}

#[test]
fn packets_flow_from_server_to_client() {
    let (mut server, mut client) = connected_pair();
    let client_id = client.get_unique_id();

    let payload = b"hello-client".to_vec();
    server
        .send_packet(
            client_id,
            Packet {
                sender: PeerId::SERVER,
                channel: 2,
                data: payload.clone(),
                transfer_mode: TransferMode::Reliable,
            },
        )
        .expect("server sends");

    let packet = pump_for_packet(&mut server, &mut client, &payload);
    assert_eq!(packet.sender, PeerId::SERVER);
    assert_eq!(packet.channel, 2);
}

#[test]
fn large_payload_survives_extended_frame_lengths() {
    let (mut server, mut client) = connected_pair();
    let client_id = client.get_unique_id();

    // 70 KiB forces the 64-bit extended length path in the frame codec.
    let payload = vec![0xABu8; 70 * 1024];
    client
        .send_packet(
            PeerId::SERVER,
            Packet {
                sender: client_id,
                channel: 0,
                data: payload.clone(),
                transfer_mode: TransferMode::Reliable,
            },
        )
        .expect("client sends large payload");

    let packet = pump_for_packet(&mut client, &mut server, &payload);
    assert_eq!(packet.data.len(), payload.len());
}

#[test]
fn server_unreachable_peer_errors() {
    let (mut server, _client) = connected_pair();

    let err = server.send_packet(
        PeerId(999),
        Packet {
            sender: PeerId::SERVER,
            channel: 0,
            data: b"nobody".to_vec(),
            transfer_mode: TransferMode::Reliable,
        },
    );
    assert!(err.is_err(), "sending to an unknown peer must error");
}

#[test]
fn multiplayer_api_accepts_websocket_peers_on_both_ends() {
    let (server, client) = connected_pair();

    let mut api_server = MultiplayerAPI::new();
    api_server.set_multiplayer_peer(Box::new(server));
    assert!(api_server.is_server());

    let mut api_client = MultiplayerAPI::new();
    api_client.set_multiplayer_peer(Box::new(client));
    assert!(!api_client.is_server());
}

#[test]
fn two_clients_get_distinct_peer_ids() {
    let mut server = WebSocketMultiplayerPeer::new();
    let addr = server.create_server("127.0.0.1:0").expect("server binds");
    let url = format!("ws://127.0.0.1:{}/", addr.port());

    let mut first = WebSocketMultiplayerPeer::new();
    first.create_client(&url).expect("first dials");
    pump_until_connected(&mut server, &mut first);

    let mut second = WebSocketMultiplayerPeer::new();
    second.create_client(&url).expect("second dials");
    for _ in 0..200 {
        let _ = server.poll();
        let _ = first.poll();
        let _ = second.poll();
        if second.get_connection_status() == ConnectionStatus::Connected {
            break;
        }
        sleep(Duration::from_millis(5));
    }

    assert_eq!(second.get_connection_status(), ConnectionStatus::Connected);
    assert_ne!(first.get_unique_id(), second.get_unique_id());
    assert_eq!(server.get_peer_count(), 2);
}
