//! pat-s3rrv.17 — a synchronizer configured the way the Godot editor exports it.
//!
//! # What was wrong
//!
//! `MultiplayerSynchronizer` read its property list from a hand-authored
//! `_sync_properties = ["position"]` array. The real editor writes a
//! `SceneReplicationConfig` sub-resource:
//!
//! ```text
//! [sub_resource type="SceneReplicationConfig" id="..."]
//! properties/0/path = NodePath(".:position")
//! properties/0/replication_mode = 1
//!
//! [node name="Sync" type="MultiplayerSynchronizer" parent="."]
//! replication_config = SubResource("...")
//! ```
//!
//! Nothing read that, so **every synchronizer exported from the actual editor
//! replicated nothing** — and silently, because an empty property list is also
//! how "not configured yet" is spelled. The hand-authored array kept the engine's
//! own tests green while no real project could work.
//!
//! ⚠️ The second property exists to pin `replication_mode`. A reader that simply
//! collected every `properties/N/path` would pass every other assertion here and
//! still replicate a property the author marked NEVER.

use gdcore::math::Vector2;
use gdplatform::network::{MockNetwork, PeerId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Authored exactly as Godot 4 exports it — no `_sync_properties` anywhere.
const PLAYER_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="SceneReplicationConfig" id="SceneReplicationConfig_1"]
properties/0/path = NodePath(".:position")
properties/0/spawn = true
properties/0/replication_mode = 1
properties/1/path = NodePath(".:never_synced")
properties/1/spawn = true
properties/1/replication_mode = 0

[node name="Player" type="Node2D"]
position = Vector2(0, 0)
never_synced = 7

[node name="Sync" type="MultiplayerSynchronizer" parent="."]
root_path = NodePath("..")
replication_interval = 0.0
replication_config = SubResource("SceneReplicationConfig_1")
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

#[test]
fn a_scene_replication_config_drives_what_syncs() {
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

    // The spawn must have landed, or nothing below measures replication.
    assert_ne!(
        prop_at(client_loop.tree(), "/root/Main/Players/Player", "position"),
        Variant::Nil,
        "the client must have the spawned player before sync is asserted"
    );

    // Move the host's player and change BOTH properties.
    let host_player = host_loop
        .tree()
        .get_node_by_path("/root/Main/Players/Player")
        .expect("host player");
    if let Some(node) = host_loop.tree_mut().get_node_mut(host_player) {
        node.set_property("position", Variant::Vector2(Vector2::new(12.0, 34.0)));
        node.set_property("never_synced", Variant::Int(99));
    }

    for _ in 0..6 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    // The headline: a property declared ONLY in the SceneReplicationConfig
    // replicated. Before this, the editor-shaped scene synced nothing.
    assert_eq!(
        prop_at(client_loop.tree(), "/root/Main/Players/Player", "position"),
        Variant::Vector2(Vector2::new(12.0, 34.0)),
        "`properties/0/path = NodePath(\".:position\")` must drive the sync. A \
         stale (0,0) means the config was never read and the synchronizer fell \
         back to an empty list"
    );

    // ⚠️ And the NEVER property must NOT have travelled. This is the assertion a
    // reader that collects every path would fail.
    assert_eq!(
        prop_at(
            client_loop.tree(),
            "/root/Main/Players/Player",
            "never_synced"
        ),
        Variant::Int(7),
        "`replication_mode = 0` is REPLICATION_MODE_NEVER — the client must keep \
         the spawned scene's own 7, not the host's 99"
    );
}
