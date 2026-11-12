//! pat-s3rrv.17 — a host that frees a spawned node despawns the client's copy.
//!
//! # What was wrong
//!
//! `NetworkCommand::Despawn = 5` has existed in the transport since it was
//! written, with round-trip tests for the byte value — and **nothing produced or
//! consumed it**. There was no `encode_despawn_packet`, no outbound diff for
//! departed children, and no inbound handler.
//!
//! The failure mode is quiet rather than loud: the removed node simply stops
//! receiving sync frames. On the client it does not vanish, it *freezes* —
//! indistinguishable from a lagging entity, and permanent. Every enemy a host
//! ever killed stayed on every client's screen forever.
//!
//! ⚠️ This test asserts the client's count goes 1 -> 0 *after the host's goes
//! to 0*. Asserting only the final state would pass against a replicator that
//! never spawned anything at all, so the spawn is asserted first and the
//! despawn measured as a transition from it.

use gdplatform::network::{MockNetwork, PeerId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;

const PLAYER_TSCN: &str = r#"[gd_scene format=3]

[node name="Player" type="Node2D"]
position = Vector2(0, 0)

[node name="Sync" type="MultiplayerSynchronizer" parent="."]
root_path = NodePath("..")
replication_interval = 0.0
_sync_properties = ["position"]
"#;

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

fn child_count(tree: &SceneTree, path: &str) -> usize {
    tree.get_node_by_path(path)
        .and_then(|id| tree.get_node(id))
        .map(|n| n.children().len())
        .unwrap_or(0)
}

#[test]
fn freeing_a_spawned_node_removes_the_client_copy() {
    // On disk so the SPAWN packet resolves through the real loader. Absolute
    // rather than `res://`, which would need process-global path roots and make
    // this order-dependent with the rest of the binary.
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
    let players = host
        .get_node_by_path("/root/Main/Players")
        .expect("host has a Players container");
    add_packed_scene_to_tree(&mut host, players, &player_scene).expect("host instances a player");

    let mut host_loop = MainLoop::new(host);
    let mut client_loop = MainLoop::new(client);
    let dt = 1.0 / 60.0;

    // -- the spawn must land first, or the despawn assertion is vacuous -------
    for _ in 0..4 {
        host_loop.step(dt);
        client_loop.step(dt);
    }
    assert_eq!(
        child_count(client_loop.tree(), "/root/Main/Players"),
        1,
        "the client must have the spawned player BEFORE the despawn — without \
         this the 0 asserted below could mean the spawn never worked"
    );

    // -- the host frees it, as a game does when the entity dies ---------------
    let doomed = host_loop
        .tree()
        .get_node_by_path("/root/Main/Players/Player")
        .expect("host still has its player");
    host_loop
        .tree_mut()
        .remove_node(doomed)
        .expect("host removes its player");
    assert_eq!(
        child_count(host_loop.tree(), "/root/Main/Players"),
        0,
        "the host's own copy is gone immediately — removal is local"
    );

    for _ in 0..4 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    assert_eq!(
        child_count(client_loop.tree(), "/root/Main/Players"),
        0,
        "the client's mirror must be removed. A 1 here is the old behaviour: \
         the node stops receiving sync frames but never leaves the tree, so it \
         freezes on screen instead of disappearing"
    );
}

/// The despawn must not resurrect: once removed, further frames must not
/// re-announce the node back onto the client.
///
/// ⚠️ This is the failure the `announced` bookkeeping invites. The receiver
/// marks inbound spawns announced at birth to stop an echo cascade; if a
/// despawn cleared the tree node without clearing that bookkeeping, the freed
/// `NodeId` would linger in the diff set — and NodeIds are reused, so the next
/// spawned node could inherit it and be silently treated as already-announced.
#[test]
fn a_despawn_stays_despawned_across_later_frames() {
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
    let players = host
        .get_node_by_path("/root/Main/Players")
        .expect("players");
    add_packed_scene_to_tree(&mut host, players, &player_scene).expect("host instances a player");

    let mut host_loop = MainLoop::new(host);
    let mut client_loop = MainLoop::new(client);
    let dt = 1.0 / 60.0;
    for _ in 0..4 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    let doomed = host_loop
        .tree()
        .get_node_by_path("/root/Main/Players/Player")
        .expect("host still has its player");
    host_loop.tree_mut().remove_node(doomed).expect("removed");

    // Deliberately long: an echo or a re-announce adds a node per frame, so a
    // short run could sample before the second copy appears.
    for _ in 0..20 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    assert_eq!(
        child_count(client_loop.tree(), "/root/Main/Players"),
        0,
        "the client must still be empty after many frames — a non-zero count \
         means the node was re-announced after its despawn"
    );
    assert_eq!(
        child_count(host_loop.tree(), "/root/Main/Players"),
        0,
        "and the host must not have received a copy back from the client"
    );
}
