//! pat-s3rrv.17 — link-by-link probes for the scene replication chain.
//!
//! # Why these exist
//!
//! `multiplayer_spawner_and_synchronizer_replicate_from_scene` is the bead's
//! acceptance test and it failed twice with one symptom — *client child_count at
//! `/root/Main/Players` is 0* — which is what **every** break in the chain looks
//! like. The chain is seven links long:
//!
//! 1. the `.tscn` value parses,
//! 2. it reaches the node's property bag,
//! 3. `spawn_path` resolves from the spawner,
//! 4. the spawner accepts a `spawn()` with the default (unlimited) limit,
//! 5. the packet survives encode/decode **and** is not mistaken for an RPC or a
//!    raw payload by `MultiplayerAPI::poll`'s classifier,
//! 6. the transport delivers it to the other peer,
//! 7. the receiver resolves the scene path off disk and instantiates it.
//!
//! One end-to-end assertion cannot say which link broke, and the outbound path
//! swallows the failure: a spawner that errors marks the child announced anyway
//! (scene_tree.rs, `poll_replication_spawns`) so it does not retry forever —
//! correct behaviour that costs the only signal there was.
//!
//! Each test below pins exactly one link through public API. Whichever goes red
//! names the break; all green rules out links 1-7 individually and points at
//! their composition instead.

use gdplatform::network::{
    decode_raw_packet, decode_rpc_packet, decode_spawn_packet, encode_spawn_packet, MockNetwork,
    MultiplayerSpawner, NetworkPeer, Packet, PeerId, SpawnPacket, SpawnRequest, TransferMode,
};
use gdscene::multiplayer::MultiplayerReplicator;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// The same world scene the acceptance test loads.
fn world_tscn(player_scene_path: &str) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Main" type="Node"]

[node name="Spawner" type="MultiplayerSpawner" parent="."]
spawn_path = NodePath("../Players")
_spawnable_scenes = ["{player_scene_path}"]

[node name="Players" type="Node" parent="."]
"#
    )
}

fn load_tree(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

// ---------------------------------------------------------------------------
// Links 1-2: the `.tscn` array reaches the live node's property bag.
// ---------------------------------------------------------------------------

/// `_spawnable_scenes` must arrive as a `Variant::Array` of paths.
///
/// This is the link the parked diagnosis called the most likely cause: if the
/// array does not reach the bag, `register_replication_nodes` registers a
/// spawner with no tracked paths, every `spawn()` returns
/// `"no spawnable scenes registered"`, and the child is marked announced with
/// nothing sent.
///
/// ⚠️ An absolute path on purpose, matching the acceptance test — `res://`
/// would need the process-global `set_path_roots`.
#[test]
fn the_spawnable_scenes_array_reaches_the_node() {
    let tree = load_tree(&world_tscn("/tmp/player.tscn"));
    let spawner = tree
        .get_node_by_path("/root/Main/Spawner")
        .and_then(|id| tree.get_node(id))
        .expect("the world scene has a Spawner");

    match spawner.get_property("_spawnable_scenes") {
        Variant::Array(items) => {
            assert_eq!(items.len(), 1, "the one authored path must be present");
            assert_eq!(
                items[0],
                Variant::String("/tmp/player.tscn".into()),
                "and must arrive as a String, not an unparsed literal"
            );
        }
        // ⚠️ `Nil` here means the property never reached the bag at all; a
        // `String` means the array literal was stored unparsed. The two have
        // different fixes, so they are reported apart.
        other => panic!(
            "`_spawnable_scenes` must be a Variant::Array; got {other:?} — Nil \
             means the property line never reached the node, a String means the \
             `[...]` literal was not parsed"
        ),
    }
}

/// `spawn_path` must arrive as a real `NodePath`, not the literal text
/// `NodePath("../Players")`. A stringified path resolves to nothing, the
/// outbound diff `continue`s past the spawner, and no packet is ever built.
#[test]
fn the_spawn_path_arrives_as_a_node_path() {
    let tree = load_tree(&world_tscn("/tmp/player.tscn"));
    let spawner_id = tree
        .get_node_by_path("/root/Main/Spawner")
        .expect("the world scene has a Spawner");
    let value = tree
        .get_node(spawner_id)
        .expect("spawner node")
        .get_property("spawn_path");

    match value {
        Variant::NodePath(p) => assert_eq!(p.to_string(), "../Players"),
        other => panic!("`spawn_path` must be a Variant::NodePath; got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Link 3: the path resolves from the spawner to the container.
// ---------------------------------------------------------------------------

/// `../Players` is relative to the SPAWNER, and the parent hop is what makes it
/// interesting: the container is the spawner's sibling, not its child.
#[test]
fn the_spawn_path_resolves_to_the_container() {
    let tree = load_tree(&world_tscn("/tmp/player.tscn"));
    let spawner_id = tree
        .get_node_by_path("/root/Main/Spawner")
        .expect("spawner");
    let players_id = tree
        .get_node_by_path("/root/Main/Players")
        .expect("players");

    assert_eq!(
        tree.get_node_or_null(spawner_id, "../Players"),
        Some(players_id),
        "`..` must walk to the parent — an unresolved spawn_path silently \
         skips the spawner in the outbound diff"
    );
}

// ---------------------------------------------------------------------------
// Link 4: the default spawn limit does not block every spawn.
// ---------------------------------------------------------------------------

/// A `.tscn` that omits `spawn_limit` yields `0`, and Godot's `0` means
/// **unlimited**. If it were read as a cap, the very first `spawn()` would fail
/// and produce exactly the observed symptom.
#[test]
fn a_zero_spawn_limit_means_unlimited() {
    let mut spawner = MultiplayerSpawner::new(0);
    spawner.add_spawnable_scene("/tmp/player.tscn");

    for i in 0..3 {
        assert!(
            spawner.spawn("").is_ok(),
            "spawn {i} must succeed with spawn_limit = 0 (unlimited)"
        );
    }

    // And a real limit is still honoured, so the assertion above is not just
    // reporting that limits are ignored altogether.
    let mut capped = MultiplayerSpawner::new(1);
    capped.add_spawnable_scene("/tmp/player.tscn");
    assert!(capped.spawn("").is_ok());
    assert!(
        capped.spawn("").is_err(),
        "a spawn_limit of 1 must reject the second spawn"
    );
}

/// A spawner with no tracked path fails — the error the outbound pass swallows.
/// Pinned so the failure mode stays a distinguishable `Err` rather than an
/// `Ok` carrying an empty scene path, which would replicate a node the receiver
/// cannot resolve.
#[test]
fn a_spawner_with_no_scenes_fails_rather_than_spawning_nothing() {
    let mut spawner = MultiplayerSpawner::new(0);
    assert!(spawner.spawn("").is_err());
}

// ---------------------------------------------------------------------------
// Link 5: the packet survives the wire format AND the poll classifier.
// ---------------------------------------------------------------------------

/// `MultiplayerAPI::poll` tries RPC first, then raw, and only then hands the
/// packet on as "other" — which is the sole route replication frames take. A
/// spawn packet accepted by either earlier decoder would be consumed as an RPC
/// or surfaced as a `peer_packet` event, and the replication path would never
/// see it.
#[test]
fn a_spawn_packet_round_trips_and_is_not_claimed_by_another_decoder() {
    let req = SpawnRequest {
        scene_index: 0,
        scene_path: "/tmp/player.tscn".into(),
        data: String::new(),
        net_id: 7,
    };
    let encoded = encode_spawn_packet(&req);

    let decoded: SpawnPacket = decode_spawn_packet(&encoded).expect("a spawn packet decodes");
    assert_eq!(decoded.net_id, 7);
    assert_eq!(decoded.scene_path, "/tmp/player.tscn");

    assert!(
        decode_rpc_packet(&encoded).is_none(),
        "the RPC decoder runs FIRST in poll(); claiming a spawn frame here \
         would divert it into inbound_rpcs and it would never reach replication"
    );
    assert!(
        decode_raw_packet(&encoded).is_none(),
        "the raw decoder runs second; claiming it would surface the frame as a \
         peer_packet event instead"
    );
}

// ---------------------------------------------------------------------------
// Link 6: the transport actually carries it between the paired peers.
// ---------------------------------------------------------------------------

/// ⚠️ The addressing detail this pins: `replication_targets()` falls back to
/// `PeerId::SERVER` because `create_pair` fires no `PeerConnected` signals, so
/// the peer roster is empty and the **server** ends up addressing packets to
/// `SERVER` — itself. That is only harmless because `MockNetworkPeer` routes to
/// its paired `remote_inbox` regardless of `to`. If that ever changes, the
/// acceptance test breaks with the same silent zero, so it is asserted here
/// rather than assumed.
#[test]
fn the_paired_peers_deliver_regardless_of_the_addressed_id() {
    let (mut server, mut client) = MockNetwork::create_pair(PeerId(2));
    let sender = server.get_unique_id();

    server
        .send_packet(
            PeerId::SERVER, // deliberately NOT the client's id
            Packet {
                sender,
                channel: 0,
                data: vec![9, 9, 9],
                transfer_mode: TransferMode::Reliable,
            },
        )
        .expect("send succeeds");

    let received = client.poll();
    assert_eq!(
        received.len(),
        1,
        "the client must receive what the server sent — an empty inbox here \
         means replication never crosses the pair, whatever the rest does"
    );
    assert_eq!(received[0].data, vec![9, 9, 9]);
    assert!(
        server.poll().is_empty(),
        "and it must not also loop back to the sender, which would make the \
         host apply its own spawn and echo forever"
    );
}

// ---------------------------------------------------------------------------
// Link 7: the receiver resolves the path off disk and instantiates it.
// ---------------------------------------------------------------------------

/// The receiving side reads the scene through `FileAccess::read_file_string`.
/// An absolute path must work without `set_path_roots` — the acceptance test
/// uses one precisely to stay independent of that process-global state, so if
/// absolute paths did not resolve, every spawn would be dropped on arrival.
#[test]
fn a_spawn_packet_instantiates_the_scene_from_an_absolute_path() {
    const PLAYER_TSCN: &str = "[gd_scene format=3]\n\n[node name=\"Player\" type=\"Node2D\"]\n";

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("player.tscn");
    std::fs::write(&path, PLAYER_TSCN).expect("write player scene");
    let path = path.to_string_lossy().to_string();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut replicator = MultiplayerReplicator::default();

    let packet = SpawnPacket {
        scene_path: path.clone(),
        data: String::new(),
        net_id: 1,
        scene_index: 0,
    };
    let spawned = replicator.apply_spawn(&mut tree, root, &packet, |p| {
        gdcore::file_access::FileAccess::read_file_string(p).ok()
    });

    let id = spawned.expect(
        "an absolute scene path must resolve and instantiate — None means the \
         receiver drops every spawn it is sent",
    );
    assert_eq!(
        tree.get_node(id).map(|n| n.name().to_string()),
        Some("Player".to_string())
    );
    assert_eq!(
        replicator.node_for(1),
        Some(id),
        "the net_id must map to the new node, or later SYNC frames address \
         nothing and are silently dropped"
    );
}
