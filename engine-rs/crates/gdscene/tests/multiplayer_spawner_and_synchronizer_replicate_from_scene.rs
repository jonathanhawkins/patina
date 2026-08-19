//! pat-s3rrv.17 — MultiplayerSpawner/MultiplayerSynchronizer replicate from a
//! `.tscn`, driven by the SceneTree tick.
//!
//! Godot registers these nodes with the MultiplayerAPI as they enter the tree
//! (`object_configuration_add`, multiplayer_spawner.cpp
//! `_notification(NOTIFICATION_POST_ENTER_TREE)`), and
//! `SceneReplicationInterface` then emits SPAWN for scenes added under
//! `spawn_path` and SYNC at `replication_interval`, applying both on the
//! receiving peer.
//!
//! Patina had every piece and no wiring: `SceneTree::poll_multiplayer_with_delta`
//! bound the non-RPC packets to `_other` and dropped them, `MultiplayerReplicator`
//! had zero callers outside its own tests, and `MultiplayerSpawner::spawn_and_notify`
//! / `MultiplayerSynchronizer::poll_outbound` were never called at all. A `.tscn`
//! containing these nodes instantiated an inert `Node`.
//!
//! ⚠️ This test drives only scene loading and `MainLoop::step`. It never calls
//! `MultiplayerReplicator`, `poll_outbound` or `encode_sync_packet` — those are
//! the very functions that were green in isolation while the feature did not
//! exist, so touching them here would re-create the false positive.

use gdplatform::network::{MockNetwork, PeerId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// The replicated scene. Its `MultiplayerSynchronizer` replicates the parent
/// (`root_path = ".."`, Godot's default).
///
/// `velocity` is a `Vector3` and `alive` a `bool` on purpose: the old transport
/// stringified values and could only spell `Vector2` as `"x,y"`, so those two
/// are what prove the payload is a real binary Variant.
const PLAYER_TSCN: &str = r#"[gd_scene format=3]

[node name="Player" type="Node2D"]
position = Vector2(0, 0)

[node name="Sync" type="MultiplayerSynchronizer" parent="."]
root_path = NodePath("..")
replication_interval = 0.0
_sync_properties = ["position", "velocity", "alive"]
"#;

/// Host/client scene: a spawner watching a `Players` container.
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
    let scene = PackedScene::from_tscn(tscn).expect("world scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("world instances");
    tree
}

/// Reads a property off the node at `path`, or `Nil` when the node is absent.
fn prop_at(tree: &SceneTree, path: &str, name: &str) -> Variant {
    match tree.get_node_by_path(path).and_then(|id| tree.get_node(id)) {
        Some(node) => node.get_property(name),
        None => Variant::Nil,
    }
}

fn child_count(tree: &SceneTree, path: &str) -> usize {
    tree.get_node_by_path(path)
        .and_then(|id| tree.get_node(id))
        .map(|n| n.children().len())
        .unwrap_or(0)
}

/// **The acceptance test.**
#[test]
fn multiplayer_spawner_and_synchronizer_replicate_from_scene() {
    // The spawnable scene lives on disk so the SPAWN packet resolves it through
    // the real loader (`FileAccess::read_file_string`) rather than a test
    // closure. An absolute path is used deliberately: `res://` would need
    // `set_path_roots`, which is process-global and would make this test
    // order-dependent with every other test in the binary.
    let dir = tempfile::tempdir().expect("tempdir");
    let player_path = dir.path().join("player.tscn");
    std::fs::write(&player_path, PLAYER_TSCN).expect("write player scene");
    let player_path = player_path.to_string_lossy().to_string();

    let mut host = load_tree(&world_tscn(&player_path));
    let client = load_tree(&world_tscn(&player_path));

    let (server, client_peer) = MockNetwork::create_pair(PeerId(2));
    host.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server));
    let mut client = client;
    client
        .get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(client_peer));

    // The host adds a child under the spawner's `spawn_path` — ordinary scene
    // loading, exactly what a game does when it instances a player.
    let player_scene = PackedScene::from_tscn(PLAYER_TSCN).expect("player scene parses");
    let players = host
        .get_node_by_path("/root/Main/Players")
        .expect("host has a Players container");
    add_packed_scene_to_tree(&mut host, players, &player_scene).expect("host instances a player");
    assert_eq!(
        child_count(&host, "/root/Main/Players"),
        1,
        "host has the player it just added"
    );
    assert_eq!(
        child_count(&client, "/root/Main/Players"),
        0,
        "client has nothing yet — replication has not run"
    );

    let mut host_loop = MainLoop::new(host);
    let mut client_loop = MainLoop::new(client);
    let dt = 1.0 / 60.0;

    // -- spawn replicates ------------------------------------------------------
    // A few frames: the host announces on its poll, the client applies on its own.
    for _ in 0..4 {
        host_loop.step(dt);
        client_loop.step(dt);
    }
    assert_eq!(
        child_count(client_loop.tree(), "/root/Main/Players"),
        1,
        "the client must instantiate the spawned scene under the mirrored \
         spawn_path — 0 means the SPAWN packet never reached the tree"
    );
    assert_eq!(
        prop_at(client_loop.tree(), "/root/Main/Players/Player", "position"),
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        "the spawned node carries its scene's own defaults"
    );

    // -- sync replicates, as real Variants ------------------------------------
    // Move the host's player and give it values the string codec could not have
    // represented.
    {
        let tree = host_loop.tree_mut();
        let id = tree
            .get_node_by_path("/root/Main/Players/Player")
            .expect("host player exists");
        let node = tree.get_node_mut(id).expect("host player node");
        node.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(12.0, 34.0)),
        );
        node.set_property(
            "velocity",
            Variant::Vector3(gdcore::math::Vector3::new(1.0, 2.0, 3.0)),
        );
        node.set_property("alive", Variant::Bool(true));
    }

    for _ in 0..6 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    assert_eq!(
        prop_at(client_loop.tree(), "/root/Main/Players/Player", "position"),
        Variant::Vector2(gdcore::math::Vector2::new(12.0, 34.0)),
        "the moved position must reach the client as a Vector2"
    );
    // ⚠️ The assertions that prove the payload is a binary Variant. The previous
    // transport stringified every value and had exactly one spelling — `"x,y"`
    // → Vector2 — so these two could not have survived it: they would arrive as
    // `Variant::String` and fail here while `position` above still passed.
    assert_eq!(
        prop_at(client_loop.tree(), "/root/Main/Players/Player", "velocity"),
        Variant::Vector3(gdcore::math::Vector3::new(1.0, 2.0, 3.0)),
        "a Vector3 must survive the wire with its type intact"
    );
    assert_eq!(
        prop_at(client_loop.tree(), "/root/Main/Players/Player", "alive"),
        Variant::Bool(true),
        "a bool must arrive as a bool, not the string \"true\""
    );

    // The host is unchanged — replication is one-directional here, and a value
    // echoed back would show up as the client's node overwriting the host's.
    assert_eq!(
        prop_at(host_loop.tree(), "/root/Main/Players/Player", "position"),
        Variant::Vector2(gdcore::math::Vector2::new(12.0, 34.0)),
        "the host keeps the value it set"
    );
}

/// A spawner announces each child exactly once, however long it lives, and a
/// received child is never announced back.
///
/// Regression guard for two distinct ways the outbound diff can run away:
/// without the `announced` set the poll re-announces every existing child on
/// every frame, and without marking INBOUND spawns as announced the two peers
/// bounce the same node back and forth, each side gaining one per frame.
#[test]
fn spawner_announces_each_child_only_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let player_path = dir.path().join("player.tscn");
    std::fs::write(&player_path, PLAYER_TSCN).expect("write player scene");
    let player_path = player_path.to_string_lossy().to_string();

    let mut host = load_tree(&world_tscn(&player_path));
    let mut client = load_tree(&world_tscn(&player_path));
    let (server, client_peer) = MockNetwork::create_pair(PeerId(2));
    host.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server));
    client
        .get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(client_peer));

    let player_scene = PackedScene::from_tscn(PLAYER_TSCN).expect("player scene parses");
    let players = host.get_node_by_path("/root/Main/Players").unwrap();
    add_packed_scene_to_tree(&mut host, players, &player_scene).unwrap();

    let mut host_loop = MainLoop::new(host);
    let mut client_loop = MainLoop::new(client);
    for _ in 0..20 {
        host_loop.step(1.0 / 60.0);
        client_loop.step(1.0 / 60.0);
    }

    assert_eq!(
        child_count(client_loop.tree(), "/root/Main/Players"),
        1,
        "20 frames must still yield exactly one replicated child, not one per frame"
    );
    // ⚠️ The HOST side is where the echo lands FIRST, and it is a different
    // failure from the one above. A node the client received is not the
    // client's to announce: without that guard the client's own outbound diff
    // sees the freshly applied child as new and announces it back, the host
    // applies it, the host's diff announces it again — both trees gain one node
    // per frame. Asserting only the client would still fail, but one frame
    // later and for a reason that reads like a spawner bug rather than an echo.
    assert_eq!(
        child_count(host_loop.tree(), "/root/Main/Players"),
        1,
        "the host must not grow either — a received node must never be          re-announced back to its sender"
    );
}
