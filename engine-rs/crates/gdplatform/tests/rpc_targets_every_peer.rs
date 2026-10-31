//! pat-mgixk.26 (item 1): `rpc()` addresses **every** peer, not the server.
//!
//! Godot's `Node.rpc()` sends to peer `0`, which means every connected peer
//! (`scene_rpc_interface.cpp` `rpcp`). Patina routed by an `authority_map`
//! that **nothing outside `network.rs`'s own tests ever wrote**, falling back
//! to `PeerId::SERVER` — so the target was always peer 1. A server's `rpc()`
//! was addressed to itself, and `WebSocketMultiplayerPeer::send_packet`
//! answered `PeerUnreachable` for an id with no connection. `rpc()` reached
//! nobody.
//!
//! ⚠️ That bug is invisible to the obvious test. `MockNetwork::create_pair`
//! has exactly one remote inbox and **ignores the `to` address**, so a packet
//! addressed to the wrong peer still arrives and every round-trip test passes.
//! These tests opt into `set_addressed_delivery`, which is the only way to
//! observe *who a packet was addressed to* rather than merely that it landed.

use gdplatform::network::{MockNetwork, MultiplayerAPI, PeerId, RPCConfig, RPCMode, TransferMode};
use gdvariant::Variant;

const NODE_PATH: u64 = 0xBEEF;

fn config() -> RPCConfig {
    RPCConfig {
        mode: RPCMode::Any,
        transfer_mode: TransferMode::Reliable,
        channel: 0,
    }
}

/// **The core claim**, asserted in both directions through a real paired
/// exchange: the broadcast is delivered, and the same call addressed to
/// `SERVER` only is REFUSED — which is what proves the target is 0, not 1.
#[test]
fn rpc_targets_every_peer() {
    // -- accepted when the filter admits BROADCAST ------------------------
    {
        let (mut server, client) = MockNetwork::create_pair(PeerId(2));
        server.set_addressed_delivery(Some(PeerId::BROADCAST));
        let mut server_api = MultiplayerAPI::new();
        server_api.set_multiplayer_peer(Box::new(server));
        server_api.register_rpc("ping", config());
        let mut client_api = MultiplayerAPI::new();
        client_api.set_multiplayer_peer(Box::new(client));

        server_api
            .rpc(NODE_PATH, "ping", vec![Variant::Int(7)])
            .expect("send");
        let _ = client_api.poll();
        let rpcs = client_api.take_inbound_rpcs();
        assert_eq!(
            rpcs.len(),
            1,
            "an rpc() addressed to BROADCAST must reach the peer; the old code \
             addressed it to PeerId::SERVER and this filter dropped it"
        );
        assert_eq!(rpcs[0].method_name, "ping");
        assert_eq!(rpcs[0].args[0], Variant::Int(7));
    }

    // -- and REFUSED when the filter admits only SERVER -------------------
    //
    // ⚠️ This is the half that pins the direction. Without it the test above
    // would also pass if `rpc` still targeted SERVER and the filter were
    // simply ignored.
    {
        let (mut server, client) = MockNetwork::create_pair(PeerId(2));
        server.set_addressed_delivery(Some(PeerId::SERVER));
        let mut server_api = MultiplayerAPI::new();
        server_api.set_multiplayer_peer(Box::new(server));
        server_api.register_rpc("ping", config());
        let mut client_api = MultiplayerAPI::new();
        client_api.set_multiplayer_peer(Box::new(client));

        server_api
            .rpc(NODE_PATH, "ping", vec![Variant::Int(7)])
            .expect("an undeliverable datagram is still Ok, as a real transport is");
        let _ = client_api.poll();
        assert!(
            client_api.take_inbound_rpcs().is_empty(),
            "rpc() is addressed to peer 0, so a filter admitting only peer 1 \
             must drop it — if this arrives, the target is still SERVER"
        );
    }
}

/// `PeerId::BROADCAST` is 0, Godot's "all peers" sentinel, and is distinct
/// from the server id. A transport must fan it out rather than look it up.
#[test]
fn broadcast_is_peer_zero_and_not_the_server() {
    assert_eq!(PeerId::BROADCAST, PeerId(0));
    assert_ne!(PeerId::BROADCAST, PeerId::SERVER);
}
