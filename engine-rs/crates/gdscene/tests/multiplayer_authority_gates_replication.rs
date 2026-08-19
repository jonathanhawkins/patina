//! pat-s3rrv.17 — only a spawner's multiplayer authority replicates it.
//!
//! # What was wrong
//!
//! There was no authority model at all: `multiplayer_authority` did not appear
//! anywhere outside tests. **Every** peer with a spawner announced its children,
//! and the resulting echo — each peer announcing what the other had just sent —
//! was suppressed by marking inbound spawns announced-at-birth on the receiver.
//!
//! That workaround prevents the symptom without expressing the rule, and it only
//! holds while the two peers happen to disagree about who spawned first. Godot's
//! actual rule is that the node's authority replicates and nobody else does.
//!
//! ⚠️ The two tests here are a matched pair on purpose. Gating replication on
//! *anything* makes the first test pass — including a gate that switches
//! replication off entirely. The second is what proves the gate still lets the
//! default (server-authoritative) case through.

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

/// Builds the connected pair with a player already added on the host.
fn connected_pair(player_path: &str) -> (SceneTree, SceneTree) {
    let mut host = load_tree(&world_tscn(player_path));
    let mut client = load_tree(&world_tscn(player_path));

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
    (host, client)
}

fn write_player(dir: &std::path::Path) -> String {
    let player_path = dir.join("player.tscn");
    std::fs::write(&player_path, PLAYER_TSCN).expect("write player scene");
    player_path.to_string_lossy().to_string()
}

/// A host that is NOT the spawner's authority must not announce its children.
#[test]
fn a_non_authority_peer_does_not_announce() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut host, client) = connected_pair(&write_player(dir.path()));

    // Hand the spawner to the CLIENT (peer 2). The host is peer 1, so it is no
    // longer the authority and must stay quiet — even though it is the server
    // and holds the only player node.
    let spawner = host
        .get_node_by_path("/root/Main/Spawner")
        .expect("host has a spawner");
    host.set_multiplayer_authority(spawner, 2, false);
    assert_eq!(
        host.get_multiplayer_authority(spawner),
        2,
        "the authority must actually be stored, or the gate below is untested"
    );

    let mut host_loop = MainLoop::new(host);
    let mut client_loop = MainLoop::new(client);
    let dt = 1.0 / 60.0;
    for _ in 0..6 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    assert_eq!(
        child_count(host_loop.tree(), "/root/Main/Players"),
        1,
        "the host keeps its own node — authority governs REPLICATION, not local \
         scene contents"
    );
    assert_eq!(
        child_count(client_loop.tree(), "/root/Main/Players"),
        0,
        "the client must receive nothing: peer 1 is not the spawner's authority. \
         A 1 here means the announcement ignored authority entirely"
    );
}

/// The control: with no authority authored, the server is the authority and
/// replication works exactly as before.
///
/// Without this, a gate that simply disabled all replication would pass the
/// test above.
#[test]
fn the_default_authority_is_the_server_and_still_replicates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (host, client) = connected_pair(&write_player(dir.path()));

    // Nothing sets `multiplayer_authority` anywhere in this scene.
    let spawner = host
        .get_node_by_path("/root/Main/Spawner")
        .expect("host has a spawner");
    assert_eq!(
        host.get_multiplayer_authority(spawner),
        1,
        "an unauthored node inherits the server as its authority, matching Godot"
    );

    let mut host_loop = MainLoop::new(host);
    let mut client_loop = MainLoop::new(client);
    let dt = 1.0 / 60.0;
    for _ in 0..6 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    assert_eq!(
        child_count(client_loop.tree(), "/root/Main/Players"),
        1,
        "the default case must still replicate — a 0 here means the authority \
         gate is rejecting the server too, i.e. it disabled replication rather \
         than scoping it"
    );
}
