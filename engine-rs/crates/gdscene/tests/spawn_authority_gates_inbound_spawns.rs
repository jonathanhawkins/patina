//! pat-s3rrv.17 item (5) — the receiver enforces the spawner's authority.
//!
//! ⚠️ **Godot does not replicate authority.** I recorded this item as "authority
//! is not replicated between peers" and went looking for a protocol addition to
//! carry it. The C++ says otherwise: `scene_replication_interface.cpp:577` is
//! `ERR_FAIL_COND_V(p_from != spawner->get_multiplayer_authority(),
//! ERR_UNAUTHORIZED)`. Both peers already agree on the spawner's authority
//! because the spawner is part of the shared scene; what was missing here was
//! the receiver *enforcing* it. Nothing goes on the wire.
//!
//! That also unblocked item (4). The receiver used to mark inbound spawns
//! "announced at birth" to stop an echo cascade — a received child looked new to
//! the outbound diff, which announced it back to the sender, which applied and
//! re-announced it, one node per frame on both trees forever. With the gate in
//! place the rule expresses itself: an accepted spawn came from the spawner's
//! authority, and a peer never receives its own packets, so the receiver is by
//! construction not that authority and skips the spawner entirely.

use gdplatform::network::{MockNetwork, PeerId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;

const PLAYER_TSCN: &str = r#"[gd_scene format=3]

[node name="Player" type="Node2D"]
position = Vector2(0, 0)

[node name="Sync" type="MultiplayerSynchronizer" parent="."]
root_path = NodePath("..")
replication_interval = 100.0
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

/// Gives the Spawner node an explicit authority on one tree.
fn set_spawner_authority(tree: &mut SceneTree, authority: i64) {
    let spawner = tree
        .get_node_by_path("/root/Main/Spawner")
        .expect("the world has a Spawner");
    tree.set_multiplayer_authority(spawner, authority, true);
}

fn add_player(tree: &mut SceneTree) {
    let scene = PackedScene::from_tscn(PLAYER_TSCN).expect("player scene parses");
    let players = tree
        .get_node_by_path("/root/Main/Players")
        .expect("the world has a Players container");
    add_packed_scene_to_tree(tree, players, &scene).expect("instances a player");
}

/// Builds the two trees, wires a peer pair, and returns them as loops.
fn linked_pair(player_path: &str) -> (SceneTree, SceneTree) {
    let mut server = load_tree(&world_tscn(player_path));
    let mut client = load_tree(&world_tscn(player_path));
    let (server_peer, client_peer) = MockNetwork::create_pair(PeerId(2));
    server
        .get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server_peer));
    client
        .get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(client_peer));
    (server, client)
}

fn player_scene_on_disk(dir: &std::path::Path) -> String {
    let path = dir.join("player.tscn");
    std::fs::write(&path, PLAYER_TSCN).expect("write player scene");
    path.to_string_lossy().to_string()
}

/// 🔑 A spawn from a peer that is not the spawner's authority is REJECTED.
///
/// The client is told it owns the spawner; the server is not. So the client
/// announces a spawn and the server sees `from = 2` against its own view of the
/// authority, `1`. Godot answers that with `ERR_UNAUTHORIZED`.
///
/// ⚠️ Read together with [`an_authorized_spawn_from_a_client_is_applied`]. On its
/// own this test is also satisfied by a gate that rejects *every* inbound spawn,
/// or by a client that never sent anything; the pair differ only in whether the
/// SERVER agrees about the authority, so it is the gate that separates them.
#[test]
fn an_unauthorized_spawn_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let player_path = player_scene_on_disk(dir.path());
    let (server, mut client) = linked_pair(&player_path);

    // Only the CLIENT believes it owns the spawner — the disagreement is the
    // whole point.
    set_spawner_authority(&mut client, 2);
    add_player(&mut client);

    let mut server_loop = MainLoop::new(server);
    let mut client_loop = MainLoop::new(client);
    for _ in 0..6 {
        client_loop.step(1.0 / 60.0);
        server_loop.step(1.0 / 60.0);
    }

    assert_eq!(
        child_count(client_loop.tree(), "/root/Main/Players"),
        1,
        "the client must still hold its own player — 0 would mean the spawn \
         never happened locally and the server having nothing proves nothing",
    );
    assert_eq!(
        child_count(server_loop.tree(), "/root/Main/Players"),
        0,
        "the server must REFUSE a spawn from a peer it does not consider the \
         spawner's authority; 1 means the packet was applied unchecked, which \
         lets any peer create nodes in everyone else's tree",
    );
}

/// 🔑 The control: when both peers agree the client owns the spawner, it works.
///
/// Without this, a gate that simply dropped all inbound spawns would pass the
/// rejection test above and break replication entirely.
#[test]
fn an_authorized_spawn_from_a_client_is_applied() {
    let dir = tempfile::tempdir().expect("tempdir");
    let player_path = player_scene_on_disk(dir.path());
    let (mut server, mut client) = linked_pair(&player_path);

    // Both sides agree: peer 2 owns the spawner. That is how a Godot game hands
    // a client authority — the same assignment runs on every peer.
    set_spawner_authority(&mut server, 2);
    set_spawner_authority(&mut client, 2);
    add_player(&mut client);

    let mut server_loop = MainLoop::new(server);
    let mut client_loop = MainLoop::new(client);
    for _ in 0..6 {
        client_loop.step(1.0 / 60.0);
        server_loop.step(1.0 / 60.0);
    }

    assert_eq!(
        child_count(server_loop.tree(), "/root/Main/Players"),
        1,
        "with the authority agreed, the server must apply the client's spawn",
    );
}

/// 🔑 A received spawn is never echoed back — the guard for removing the
/// announced-at-birth workaround (item 4).
///
/// ⚠️ This passes against the OLD code too, and that is deliberate: it is a
/// regression guard for a *removal*, not a discriminator. Its job is to fail if
/// the reasoning that made the workaround redundant is wrong. Twenty frames is
/// well past the point where an echo would show — the cascade adds a node per
/// frame to BOTH trees, so even one round trip would put these counts at 3+.
#[test]
fn a_received_spawn_is_not_echoed_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let player_path = player_scene_on_disk(dir.path());
    let (mut server, client) = linked_pair(&player_path);

    // The ordinary case: the server owns the spawner by default and spawns.
    add_player(&mut server);

    let mut server_loop = MainLoop::new(server);
    let mut client_loop = MainLoop::new(client);
    for _ in 0..20 {
        server_loop.step(1.0 / 60.0);
        client_loop.step(1.0 / 60.0);
    }

    assert_eq!(
        child_count(client_loop.tree(), "/root/Main/Players"),
        1,
        "the client must hold exactly one player: 0 means the spawn never \
         arrived, more than 1 means it is echoing",
    );
    assert_eq!(
        child_count(server_loop.tree(), "/root/Main/Players"),
        1,
        "and the server must not have grown a copy of its own spawn coming \
         back — that is the cascade the removed workaround used to suppress",
    );
}
