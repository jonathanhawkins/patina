//! pat-s3rrv.177 — `replication_interval` is SECONDS and must gate the periodic
//! sync.
//!
//! Godot ground truth: `MultiplayerSynchronizer.replication_interval` is a float
//! in **seconds** (`doc_classes/MultiplayerSynchronizer.xml`). Patina's
//! synchronizer counts **milliseconds**, so `scene_tree.rs` converts with
//! `(f * 1000.0) as u32`.
//!
//! 🔑 **That conversion had no test.** Twelve existing fixtures set
//! `replication_interval = 0.0`, where `× 1000` is a no-op and the scale term
//! vanishes entirely. The two fixtures that do use a non-zero value (100.0) do
//! not depend on it — `spawn_state_reaches_the_client`'s own header says the
//! SPAWN path is isolated by `sync = false`, not by the interval. So dropping the
//! `× 1000` would have turned every `1.0` second into 1 millisecond and **not one
//! test would have failed**.
//!
//! ⚠️ The gate is `!force && self.last_sync_ms > 0 && …` (`network.rs`), so it
//! only applies **after the first sync** — the first one fires immediately
//! whatever the interval. The test therefore lets one sync land before measuring
//! suppression, or it would be measuring the wrong thing.
//!
//! ⚠️ The clock advances `(delta * 1000.0) as u64` per frame — about 16 ms at
//! 1/60. The fixture uses a 1.0 s interval so the two readings are far apart:
//! ~128 ms of pumping must be suppressed, and ~1.2 s must get through. A dropped
//! `× 1000` makes the interval 1 ms and the suppression assertion fails.

use gdcore::math::Vector2;
use gdplatform::network::{MockNetwork, PeerId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// `position` syncs every tick (ALWAYS), behind a **one second** interval.
const PLAYER_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="SceneReplicationConfig" id="SceneReplicationConfig_1"]
properties/0/path = NodePath(".:position")
properties/0/spawn = true
properties/0/replication_mode = 1

[node name="Player" type="Node2D"]
position = Vector2(0, 0)

[node name="Sync" type="MultiplayerSynchronizer" parent="."]
root_path = NodePath("..")
replication_interval = 1.0
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

fn client_position(tree: &SceneTree) -> Variant {
    match tree
        .get_node_by_path(PLAYER)
        .and_then(|id| tree.get_node(id))
    {
        Some(node) => node.get_property("position"),
        None => Variant::Nil,
    }
}

/// A one-second interval suppresses a mid-interval change and releases it after.
///
/// ⚠️ Both halves are required. The suppression assertion alone would also pass
/// if replication had simply stopped; the release assertion is what proves the
/// value was merely *delayed*.
#[test]
fn a_one_second_interval_delays_the_next_sync_but_does_not_drop_it() {
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
        .expect("host Players container");
    let spawned = add_packed_scene_to_tree(&mut host, players, &player_scene).expect("instances");
    if let Some(node) = host.get_node_mut(spawned) {
        node.set_property("position", Variant::Vector2(Vector2::new(1.0, 1.0)));
    }

    let mut host_loop = MainLoop::new(host);
    let mut client_loop = MainLoop::new(client);
    let dt = 1.0 / 60.0;

    // Let the spawn land and the FIRST sync fire — the gate needs
    // `last_sync_ms > 0` before it suppresses anything.
    for _ in 0..8 {
        host_loop.step(dt);
        client_loop.step(dt);
    }
    assert_eq!(
        client_position(client_loop.tree()),
        Variant::Vector2(Vector2::new(1.0, 1.0)),
        "precondition: the first sync must land, or the interval is not what the \
         assertions below are measuring"
    );

    // Change it, then pump only ~128 ms — far inside a one-second interval.
    let host_player = host_loop
        .tree()
        .get_node_by_path(PLAYER)
        .expect("host player");
    if let Some(node) = host_loop.tree_mut().get_node_mut(host_player) {
        node.set_property("position", Variant::Vector2(Vector2::new(2.0, 2.0)));
    }
    for _ in 0..8 {
        host_loop.step(dt);
        client_loop.step(dt);
    }
    assert_eq!(
        client_position(client_loop.tree()),
        Variant::Vector2(Vector2::new(1.0, 1.0)),
        "⚠️ ~128 ms into a 1.0 SECOND interval, the update must still be held. \
         Seeing (2,2) means `replication_interval` was read as 1 MILLISECOND — \
         i.e. the `* 1000.0` seconds-to-ms conversion in scene_tree.rs was lost"
    );

    // Now pass the interval — ~1.2 s total — and it must arrive.
    for _ in 0..72 {
        host_loop.step(dt);
        client_loop.step(dt);
    }
    assert_eq!(
        client_position(client_loop.tree()),
        Variant::Vector2(Vector2::new(2.0, 2.0)),
        "control: once the interval elapses the update must arrive. If this is \
         still (1,1), replication stopped altogether and the suppression \
         assertion above proved nothing"
    );
}
