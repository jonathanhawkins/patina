//! pat-s3rrv.177 — spawn state is gated on `spawn` ALONE, never on the mode.
//!
//! Godot ground truth: `SceneReplicationConfig::_update()`
//! (`modules/multiplayer/scene_replication_config.cpp`) is
//!
//! ```cpp
//! if (prop.spawn) { spawn_props.push_back(prop.name); }
//! switch (prop.mode) {
//!     case REPLICATION_MODE_ALWAYS:    sync_props.push_back(prop.name); break;
//!     case REPLICATION_MODE_ON_CHANGE: watch_props.push_back(prop.name); break;
//!     default: break;
//! }
//! ```
//!
//! 🔑 The `spawn` push is **outside** the switch. `mode` decides sync vs watch vs
//! neither; it says nothing about spawn. So `spawn = true` with
//! `replication_mode = 0` (NEVER) is a coherent, ordinary configuration: the
//! value travels once in the spawn packet and is never synced again — exactly how
//! you author immutable per-instance data such as a team colour or a seed.
//!
//! ⚠️ The reader also required `mode != 0` for spawn, so that one combination was
//! silently dropped from the spawn state and never reached the client at all.
//!
//! ⚠️ This fixture is the only shape that can catch it. `mode = 1` and `mode = 2`
//! both already satisfied the old `mode != 0`, so a config using either passes
//! whether or not the bug is present.

use gdplatform::network::{MockNetwork, PeerId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// `team` is spawn-state-only: `spawn = true`, mode NEVER.
/// `hp` is the control — spawn + ALWAYS — so a total replication failure cannot
/// masquerade as correct handling of `team`.
const PLAYER_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="SceneReplicationConfig" id="SceneReplicationConfig_1"]
properties/0/path = NodePath(".:hp")
properties/0/spawn = true
properties/0/replication_mode = 1
properties/1/path = NodePath(".:team")
properties/1/spawn = true
properties/1/replication_mode = 0

[node name="Player" type="Node2D"]
hp = 10
team = 0

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

const PLAYER: &str = "/root/Main/Players/Player";

fn prop_at(tree: &SceneTree, name: &str) -> Variant {
    match tree
        .get_node_by_path(PLAYER)
        .and_then(|id| tree.get_node(id))
    {
        Some(node) => node.get_property(name),
        None => Variant::Nil,
    }
}

/// A NEVER-mode property flagged for spawn still reaches the client's spawn
/// state — and still never syncs afterwards.
#[test]
fn a_never_mode_property_is_still_spawn_state() {
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

    // ⚠️ Both values are set on the host BEFORE it spawns the player, so they are
    // part of the spawn state rather than a later sync. Setting them afterwards
    // would let the ALWAYS property arrive by sync and prove nothing about spawn.
    let player_scene = PackedScene::from_tscn(PLAYER_TSCN).expect("player scene parses");
    let players = host
        .get_node_by_path("/root/Main/Players")
        .expect("host has a Players container");
    let spawned =
        add_packed_scene_to_tree(&mut host, players, &player_scene).expect("host instances");
    if let Some(node) = host.get_node_mut(spawned) {
        node.set_property("team", Variant::Int(7));
        node.set_property("hp", Variant::Int(99));
    }

    let mut host_loop = MainLoop::new(host);
    let mut client_loop = MainLoop::new(client);
    let dt = 1.0 / 60.0;
    for _ in 0..8 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    assert!(
        client_loop.tree().get_node_by_path(PLAYER).is_some(),
        "precondition: the client must have the spawned player"
    );

    // The headline: `spawn = true, replication_mode = 0`.
    assert_eq!(
        prop_at(client_loop.tree(), "team"),
        Variant::Int(7),
        "`spawn = true` puts a property in the spawn state regardless of mode — \
         upstream pushes it OUTSIDE the mode switch. A stale 0 means the reader \
         still requires `mode != 0` for spawn and dropped it"
    );

    // Control: without this, a client that received nothing at all would satisfy
    // nothing above but would also not distinguish the two properties.
    assert_eq!(
        prop_at(client_loop.tree(), "hp"),
        Variant::Int(99),
        "control: the ALWAYS property must arrive too; if this is 10 the spawn \
         never carried state and the assertion above proves nothing"
    );
}
