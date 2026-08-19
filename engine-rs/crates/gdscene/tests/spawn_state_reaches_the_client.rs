//! pat-s3rrv.17 item (1) — spawn-flagged properties travel in the SPAWN packet.
//!
//! `SceneReplicationConfig::spawn_properties()` recorded which properties are
//! spawn state and **nothing ever sent them**: the accessor had exactly one
//! caller repo-wide and it was a test assertion. So a replicated node arrived on
//! remote peers showing its scene's authored defaults, and only started matching
//! the host once the first periodic SYNC frame landed — for a
//! `replication_interval` of a second, that is a second of a visibly wrong
//! position, skin or health bar.
//!
//! ⚠️ Every config entry here sets **`sync = false`**, which is what isolates
//! the SPAWN path: with nothing flagged for sync, `poll_outbound` returns early
//! on an empty property set and no SYNC packet is ever built, so a value the
//! client holds can only have arrived in the SPAWN packet.
//!
//! ⚠️ A large `replication_interval` does NOT achieve that, and my first
//! attempt at this test assumed it did. The interval gate is
//! `!force && self.last_sync_ms > 0 && …` (`network.rs:1938`) — it only applies
//! **after the first sync**, so the first one fires immediately whatever the
//! interval says. The control test below is what caught it; the headline test
//! had been passing on a value SYNC delivered.
//!
//! ⚠️ The config is authored in the flattened
//! `__replication__properties/N/*` form, which is what the Godot editor exports.
//! The `_sync_properties` array the sibling test uses is a hand-authored SYNC
//! list and is deliberately NOT consulted for spawn state.

use gdplatform::network::{MockNetwork, PeerId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// The replicated scene. `position` is spawn state; `hp` is spawn state too and
/// is an `Int`, so a stringifying encoder cannot pass this test.
const PLAYER_TSCN: &str = r#"[gd_scene format=3]

[node name="Player" type="Node2D"]
position = Vector2(0, 0)
hp = 0

[node name="Sync" type="MultiplayerSynchronizer" parent="."]
root_path = NodePath("..")
replication_interval = 100.0
__replication__properties/0/path = ".:position"
__replication__properties/0/spawn = true
__replication__properties/0/sync = false
__replication__properties/1/path = ".:hp"
__replication__properties/1/spawn = true
__replication__properties/1/sync = false
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

/// 🔑 The spawned node arrives already carrying the host's values.
#[test]
fn spawn_state_reaches_the_client() {
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

    // The host instances a player and moves it BEFORE the spawn is announced —
    // which is the ordinary case: a game positions a player as it creates it.
    let player_scene = PackedScene::from_tscn(PLAYER_TSCN).expect("player scene parses");
    let players = host
        .get_node_by_path("/root/Main/Players")
        .expect("host has a Players container");
    add_packed_scene_to_tree(&mut host, players, &player_scene).expect("host instances a player");
    {
        let id = host
            .get_node_by_path("/root/Main/Players/Player")
            .expect("host player exists");
        let node = host.get_node_mut(id).expect("host player node");
        node.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(12.0, 34.0)),
        );
        node.set_property("hp", Variant::Int(77));
    }

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
        "the client must instantiate the spawned scene — 0 means the SPAWN \
         packet never reached the tree and nothing below is meaningful",
    );

    // ⚠️ These values cannot have come from a SYNC frame: every entry is
    // `sync = false`, so `poll_outbound` never builds one.
    assert_eq!(
        prop_at(client_loop.tree(), "/root/Main/Players/Player", "position"),
        Variant::Vector2(gdcore::math::Vector2::new(12.0, 34.0)),
        "the spawned node must arrive carrying the host's position; \
         Vector2(0, 0) is the scene default, which is what an empty spawn \
         state produces",
    );
    assert_eq!(
        prop_at(client_loop.tree(), "/root/Main/Players/Player", "hp"),
        Variant::Int(77),
        "…and an Int must survive as an Int — a stringifying encoder would \
         deliver Variant::String(\"77\") here",
    );
}

/// The control: with nothing flagged for spawn, the client gets scene defaults.
///
/// Without this, the test above would pass against an implementation that
/// ignored the config and simply shipped every property it could find.
const PLAYER_NO_SPAWN_TSCN: &str = r#"[gd_scene format=3]

[node name="Player" type="Node2D"]
position = Vector2(0, 0)
hp = 0

[node name="Sync" type="MultiplayerSynchronizer" parent="."]
root_path = NodePath("..")
replication_interval = 100.0
__replication__properties/0/path = ".:position"
__replication__properties/0/spawn = false
__replication__properties/0/sync = false
"#;

#[test]
fn a_property_not_flagged_for_spawn_is_not_sent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let player_path = dir.path().join("player.tscn");
    std::fs::write(&player_path, PLAYER_NO_SPAWN_TSCN).expect("write player scene");
    let player_path = player_path.to_string_lossy().to_string();

    let mut host = load_tree(&world_tscn(&player_path));
    let mut client = load_tree(&world_tscn(&player_path));
    let (server, client_peer) = MockNetwork::create_pair(PeerId(2));
    host.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server));
    client
        .get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(client_peer));

    let player_scene = PackedScene::from_tscn(PLAYER_NO_SPAWN_TSCN).expect("player scene parses");
    let players = host
        .get_node_by_path("/root/Main/Players")
        .expect("host has a Players container");
    add_packed_scene_to_tree(&mut host, players, &player_scene).expect("host instances a player");
    {
        let id = host
            .get_node_by_path("/root/Main/Players/Player")
            .expect("host player exists");
        let node = host.get_node_mut(id).expect("host player node");
        node.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(12.0, 34.0)),
        );
    }

    let mut host_loop = MainLoop::new(host);
    let mut client_loop = MainLoop::new(client);
    for _ in 0..6 {
        host_loop.step(1.0 / 60.0);
        client_loop.step(1.0 / 60.0);
    }

    assert_eq!(
        child_count(client_loop.tree(), "/root/Main/Players"),
        1,
        "the spawn itself must still replicate",
    );
    assert_eq!(
        prop_at(client_loop.tree(), "/root/Main/Players/Player", "position"),
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        "`spawn = false` must keep the property out of the SPAWN packet — the \
         client should show the scene default",
    );
}
