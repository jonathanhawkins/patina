//! pat-s3rrv.17 — a `SceneReplicationConfig` path that targets a CHILD node.
//!
//! # What was wrong
//!
//! Godot's editor authors replication paths as `<node>:<property>`, and the
//! node part is very often a child:
//!
//! ```text
//! properties/0/path = NodePath(".:position")
//! properties/1/path = NodePath("Sprite:modulate")
//! ```
//!
//! Patina read the config, split the path, and then **silently dropped every
//! entry whose node part was not `.`**:
//!
//! ```text
//! if !node_part.is_empty() && node_part != "." { continue; }
//! ```
//!
//! Skipping was the safe half of the decision — writing a child's `modulate`
//! onto its parent would be a silent corruption, which is worse. But it left a
//! real exported scene replicating only the subset of its properties that
//! happened to live on the synchronizer's own root, with no error anywhere. A
//! project whose config was entirely child-scoped replicated *nothing* while
//! looking correctly configured in the editor.
//!
//! # Why this test could not pass before
//!
//! The child's property is asserted on the CLIENT. Under the old reader the
//! entry never entered the sync list, so `modulate` stayed at the spawned
//! scene's own default forever — the assertion below fails on the old code
//! rather than passing vacuously.
//!
//! ⚠️ The default and the replicated value differ in three channels on purpose.
//! A fixture that replicated `Color(1,1,1,1)` onto a node already holding
//! `Color(1,1,1,1)` is green whether or not anything travelled.

use gdcore::math::{Color, Vector2};
use gdplatform::network::{MockNetwork, PeerId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Authored the way the editor exports it: one root-scoped path and one
/// child-scoped path in the same config, so the test pins that supporting
/// children did not cost the root case.
const PLAYER_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="SceneReplicationConfig" id="SceneReplicationConfig_1"]
properties/0/path = NodePath(".:position")
properties/0/replication_mode = 1
properties/1/path = NodePath("Sprite:modulate")
properties/1/replication_mode = 1

[node name="Player" type="Node2D"]
position = Vector2(0, 0)

[node name="Sprite" type="Node2D" parent="."]
modulate = Color(1, 1, 1, 1)

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
fn a_config_path_targeting_a_child_node_replicates_to_that_child() {
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

    // The spawn must have landed, and it must have brought the CHILD with it —
    // otherwise the child assertion below would be measuring a missing node
    // rather than a missing sync.
    assert_ne!(
        prop_at(
            client_loop.tree(),
            "/root/Main/Players/Player/Sprite",
            "modulate"
        ),
        Variant::Nil,
        "the client must have the spawned player's Sprite child before sync is \
         asserted; a Nil here means the spawn itself did not arrive"
    );

    // Write BOTH the root's property and the child's on the host.
    let host_player = host_loop
        .tree()
        .get_node_by_path("/root/Main/Players/Player")
        .expect("host player");
    if let Some(node) = host_loop.tree_mut().get_node_mut(host_player) {
        node.set_property("position", Variant::Vector2(Vector2::new(12.0, 34.0)));
    }
    let host_sprite = host_loop
        .tree()
        .get_node_by_path("/root/Main/Players/Player/Sprite")
        .expect("host player's Sprite child");
    if let Some(node) = host_loop.tree_mut().get_node_mut(host_sprite) {
        node.set_property("modulate", Variant::Color(Color::new(0.25, 0.5, 0.75, 1.0)));
    }

    for _ in 0..6 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    // **The headline.** `NodePath("Sprite:modulate")` must land on the Sprite.
    assert_eq!(
        prop_at(
            client_loop.tree(),
            "/root/Main/Players/Player/Sprite",
            "modulate"
        ),
        Variant::Color(Color::new(0.25, 0.5, 0.75, 1.0)),
        "a child-scoped config path must replicate to the CHILD. Still \
         Color(1,1,1,1) means the entry was dropped by the reader"
    );

    // ...and it must NOT have been written onto the root instead. This is the
    // assertion that fails if a future change "fixes" child paths by flattening
    // them onto the synchronizer's root.
    //
    // ⚠️ Deliberately `assert_ne!` against the replicated value rather than
    // `assert_eq!(.., Nil)`. `CanvasItem` registers `modulate` with a
    // `Color::WHITE` default and `packed_scene` seeds every ClassDB default onto
    // an instantiated node, so the root may legitimately hold WHITE here. What
    // must never be true is that it holds the CHILD's replicated value.
    assert_ne!(
        prop_at(client_loop.tree(), "/root/Main/Players/Player", "modulate"),
        Variant::Color(Color::new(0.25, 0.5, 0.75, 1.0)),
        "the child's property must not be flattened onto the synchronizer's root"
    );

    // The root-scoped entry in the same config still works — supporting
    // children must not have cost the case that already worked.
    assert_eq!(
        prop_at(client_loop.tree(), "/root/Main/Players/Player", "position"),
        Variant::Vector2(Vector2::new(12.0, 34.0)),
        "`.:position` must still replicate alongside the child path"
    );
}
