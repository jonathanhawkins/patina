//! pat-s3rrv.17 — `public_visibility` on a `.tscn` synchronizer is honoured.
//!
//! # What was wrong
//!
//! Per-peer visibility was fully implemented at the network layer and fully
//! unreachable from a scene. `MultiplayerSynchronizer::poll_outbound` already
//! gates every send:
//!
//! ```text
//! // Godot per-peer visibility: a peer this synchronizer is hidden from
//! // receives no sync packets.
//! if !self.is_visible_to(to) { return Ok(false); }
//! ```
//!
//! ...and `set_visibility_public` / `set_visibility_for` / `is_visible_to` all
//! existed with unit tests. But **nothing in `gdscene` ever called them** —
//! `grep -rn "is_visible_to\|visibility" crates/gdscene/src` returned nothing.
//! `register_replication_nodes` read `root_path`, `replication_interval` and
//! the property list off the node and stopped there.
//!
//! So a scene authored with `public_visibility = false` replicated to every
//! peer anyway. The failure is silent and inverted: the author asks for less
//! replication and gets all of it.
//!
//! # Why both tests exist
//!
//! The hidden case asserts a NON-event (the client's copy never changes), and a
//! non-event is exactly what a broken fixture also produces — a typo'd property
//! name, a scene that never spawned, or a synchronizer that syncs nothing at all
//! would all "pass". The public case is the control: same scene, same frames,
//! one flag flipped, and the value must arrive. Only the pair distinguishes
//! "visibility gated it" from "nothing was ever going to arrive".

use gdcore::math::Vector2;
use gdplatform::network::{MockNetwork, PeerId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

fn player_tscn(public_visibility: bool) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Player" type="Node2D"]
position = Vector2(0, 0)

[node name="Sync" type="MultiplayerSynchronizer" parent="."]
root_path = NodePath("..")
replication_interval = 0.0
public_visibility = {public_visibility}
_sync_properties = ["position"]
"#
    )
}

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

/// Spawns a player, moves it on the host, and returns the CLIENT's view of the
/// replicated `position` after enough frames for a sync to arrive.
fn client_position_after_host_moves(public_visibility: bool) -> Variant {
    let tscn = player_tscn(public_visibility);
    let dir = tempfile::tempdir().expect("tempdir");
    let player_path = dir.path().join("player.tscn");
    std::fs::write(&player_path, &tscn).expect("write player scene");
    let player_path = player_path.to_string_lossy().to_string();

    let mut host = load_tree(&world_tscn(&player_path));
    let mut client = load_tree(&world_tscn(&player_path));

    let (server, client_peer) = MockNetwork::create_pair(PeerId(2));
    host.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server));
    client
        .get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(client_peer));

    let player_scene = PackedScene::from_tscn(&tscn).expect("player scene parses");
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

    // ⚠️ Visibility gates SYNC, not SPAWN — the node must arrive either way.
    // Without this the hidden case could pass because nothing ever spawned.
    assert_ne!(
        prop_at(client_loop.tree(), "/root/Main/Players/Player", "position"),
        Variant::Nil,
        "the client must have the spawned player regardless of visibility; \
         `public_visibility` hides sync frames, not the spawn itself"
    );

    let host_player = host_loop
        .tree()
        .get_node_by_path("/root/Main/Players/Player")
        .expect("host player");
    if let Some(node) = host_loop.tree_mut().get_node_mut(host_player) {
        node.set_property("position", Variant::Vector2(Vector2::new(12.0, 34.0)));
    }

    for _ in 0..6 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    prop_at(client_loop.tree(), "/root/Main/Players/Player", "position")
}

/// The control. Same scene, visibility left at Godot's `true` default.
#[test]
fn a_public_synchronizer_replicates() {
    assert_eq!(
        client_position_after_host_moves(true),
        Variant::Vector2(Vector2::new(12.0, 34.0)),
        "with public_visibility = true the move must arrive — if this fails the \
         fixture is broken and the hidden case below proves nothing"
    );
}

/// The headline: hidden from its peers, the synchronizer sends nothing.
#[test]
fn a_non_public_synchronizer_sends_nothing() {
    assert_eq!(
        client_position_after_host_moves(false),
        Variant::Vector2(Vector2::new(0.0, 0.0)),
        "with public_visibility = false the client must keep the spawned \
         scene's own (0,0). Seeing (12,34) is the old behaviour: the flag was \
         parsed into the node and then never reached the synchronizer"
    );
}
