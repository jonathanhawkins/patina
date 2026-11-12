//! pat-s3rrv.17 — freeing the SPAWNER retracts what it announced.
//!
//! # What was wrong
//!
//! Despawn was driven purely by diffing a spawner's `spawn_path` children
//! against what it had announced. That covers "a child died", but Godot also
//! despawns a spawner's replicated nodes when the **spawner itself** leaves the
//! tree (`multiplayer_spawner.cpp`, on exiting the tree).
//!
//! In Patina the freed spawner didn't announce anything — it produced *no
//! signal at all*. The diff resolved `spawn_path` relative to the now-missing
//! spawner, got `None`, and hit the deliberate:
//!
//! ```text
//! // ⚠️ A missing parent is NOT an empty child list. [...] Skip instead.
//! let Some(parent) = parent else { continue; };
//! ```
//!
//! That skip is correct for an unresolvable path — treating it as "no children"
//! would despawn the world on a client whose scene resolves the path
//! differently. But it also swallowed the freed-spawner case, so every node the
//! spawner had announced stayed on the client forever.
//!
//! # The distinction being pinned
//!
//! A freed SPAWNER means "retract everything it announced". An unresolvable
//! `spawn_path` means "I cannot tell". Conflating them is a much worse bug than
//! the one being fixed, so the second test below pins that the host's own nodes
//! are untouched — only the remote mirror is retracted.

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

/// Builds a connected host/client pair with one player already replicated.
fn replicated_pair() -> (MainLoop, MainLoop, tempfile::TempDir, f64) {
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
    for _ in 0..4 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    // Without this the 0 asserted later could mean the spawn never worked.
    assert_eq!(
        child_count(client_loop.tree(), "/root/Main/Players"),
        1,
        "the client must have the spawned player before the spawner is freed"
    );

    (host_loop, client_loop, dir, dt)
}

#[test]
fn freeing_the_spawner_despawns_everything_it_announced() {
    let (mut host_loop, mut client_loop, _dir, dt) = replicated_pair();

    // Free the SPAWNER itself — not the spawned node.
    let spawner = host_loop
        .tree()
        .get_node_by_path("/root/Main/Spawner")
        .expect("host has its spawner");
    host_loop
        .tree_mut()
        .remove_node(spawner)
        .expect("host removes its spawner");

    for _ in 0..4 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    assert_eq!(
        child_count(client_loop.tree(), "/root/Main/Players"),
        0,
        "freeing the spawner must retract what it announced. A 1 here is the \
         old behaviour: the spawn_path lookup failed against the missing \
         spawner and the diff skipped it, so the mirror lived forever"
    );
}

/// ⚠️ The over-correction guard. A freed spawner must retract the REMOTE
/// mirror only — its spawned nodes are children of `spawn_path`, not of the
/// spawner, so Godot leaves the local ones alone.
///
/// Without this, "despawn everything when the path won't resolve" would pass
/// the test above while destroying the host's own game state.
#[test]
fn freeing_the_spawner_leaves_the_hosts_own_nodes_alone() {
    let (mut host_loop, mut client_loop, _dir, dt) = replicated_pair();

    let spawner = host_loop
        .tree()
        .get_node_by_path("/root/Main/Spawner")
        .expect("host has its spawner");
    host_loop
        .tree_mut()
        .remove_node(spawner)
        .expect("host removes its spawner");

    for _ in 0..4 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    assert_eq!(
        child_count(host_loop.tree(), "/root/Main/Players"),
        1,
        "the host's own player is a child of Players, not of the Spawner — \
         freeing the spawner must not delete local game state"
    );
}

/// Once retracted, later frames must neither resurrect the node nor panic on
/// the pruned registry entry.
#[test]
fn a_freed_spawner_stays_quiet_on_later_frames() {
    let (mut host_loop, mut client_loop, _dir, dt) = replicated_pair();

    let spawner = host_loop
        .tree()
        .get_node_by_path("/root/Main/Spawner")
        .expect("host has its spawner");
    host_loop
        .tree_mut()
        .remove_node(spawner)
        .expect("host removes its spawner");

    // Well past the despawn, so a spawner rescanned every frame would show up.
    for _ in 0..30 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    assert_eq!(
        child_count(client_loop.tree(), "/root/Main/Players"),
        0,
        "the despawn must stay despawned once the spawner's registry entry is \
         pruned"
    );
}
