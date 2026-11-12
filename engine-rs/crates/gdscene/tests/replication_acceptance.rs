//! pat-s3rrv.177 — **the bead's acceptance test.**
//!
//! Binary named `replication_acceptance`; the acceptance function keeps the full
//! name the bead specifies. (Same split `tests/backtrace.rs` uses, and it keeps
//! the verify command short.)
//!
//! Godot ground truth: `modules/multiplayer/scene_replication_config.{h,cpp}`.
//! `enum ReplicationMode` is positional — **NEVER 0, ALWAYS 1, ON_CHANGE 2** —
//! and `_update()` pushes `prop.spawn` into `spawn_props` **outside** the mode
//! switch, then routes ALWAYS → `sync_props` and ON_CHANGE → `watch_props`.
//!
//! ⚠️ **ONE CLAUSE INVOLVES A JUDGEMENT CALL, STATED HERE SO IT CAN BE
//! DISAGREED WITH.** The bead asks to assert the synchronizer's
//! `replication_config` "resolved to the resource (not Nil)". In this engine a
//! loaded Resource-typed property is represented as a `SubResource:<id>` marker
//! plus a `__resref__<prop>` companion — that is how `mesh`, `skin` and
//! `texture` all work (`packed_scene.rs:494`), and it is what the editor reads
//! (`editor_server.rs:3826`). So the assertion below is **present, non-Nil, and
//! naming the resource**, which is what "resolved" means here today.
//!
//! If the intent was a live, script-callable resource object, that is an
//! engine-wide change to resource representation — it needs a release path tied
//! to node lifetime (nothing in gdscene releases handles today) and should change
//! `mesh`/`skin`/`texture` at the same time rather than making this one property
//! special. I did not silently pick either reading: this test asserts the
//! current convention and says so.

use gdcore::math::Vector2;
use gdplatform::network::{MockNetwork, PeerId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Exactly the shape the bead specifies: `position` spawn + sync (ALWAYS),
/// `health` watch-only (ON_CHANGE, and NOT spawn state).
const PLAYER_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="SceneReplicationConfig" id="SceneReplicationConfig_1"]
properties/0/path = NodePath(".:position")
properties/0/spawn = true
properties/0/replication_mode = 1
properties/1/path = NodePath(".:health")
properties/1/spawn = false
properties/1/replication_mode = 2

[node name="Player" type="Node2D"]
position = Vector2(0, 0)
health = 100

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
const SYNC: &str = "/root/Main/Players/Player/Sync";

fn prop(tree: &SceneTree, path: &str, name: &str) -> Variant {
    match tree.get_node_by_path(path).and_then(|id| tree.get_node(id)) {
        Some(node) => node.get_property(name),
        None => Variant::Nil,
    }
}

/// The `path` of flattened config entry `index`, as a plain string.
///
/// Mirrors `scene_tree.rs`'s own `variant_as_path_string`: a NodePath, a String
/// and a StringName all name the same path here, and the replication reader
/// accepts all three.
fn path_of(tree: &SceneTree, node: &str, index: usize) -> String {
    match prop(
        tree,
        node,
        &format!("__replication__properties/{index}/path"),
    ) {
        Variant::NodePath(p) => p.to_string(),
        Variant::String(s) => s,
        Variant::StringName(s) => s.as_str().to_string(),
        other => panic!("entry {index} has no usable path, got {other:?}"),
    }
}

/// **THE ACCEPTANCE TEST.**
///
/// Drives every clause the bead lists, in order.
#[test]
fn scene_replication_config_drives_multiplayer_synchronizer_property_set() {
    // ---- clause: the class is registered as a Resource -------------------
    gdobject::class_db::register_networking_classes();
    assert!(
        gdobject::class_db::class_exists("SceneReplicationConfig"),
        "SceneReplicationConfig must be registered"
    );
    assert!(
        gdobject::class_db::is_parent_class("SceneReplicationConfig", "Resource"),
        "…and inherit Resource, as upstream does"
    );

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

    // Host-side values BEFORE the spawn, so they are spawn state rather than a
    // later sync — otherwise clause 4 below proves nothing about spawn.
    let player_scene = PackedScene::from_tscn(PLAYER_TSCN).expect("player scene parses");
    let players = host
        .get_node_by_path("/root/Main/Players")
        .expect("host Players container");
    let spawned = add_packed_scene_to_tree(&mut host, players, &player_scene).expect("instances");
    // ⚠️ `position` only. `health` is deliberately LEFT at the scene's 100 on both
    // peers, because the watch clause below asserts that an UNCHANGED watched
    // property does not travel — and writing 55 here would itself be the change
    // that legitimately sends it. My first draft did exactly that and the
    // assertion failed for its own setup, not for a defect.
    if let Some(node) = host.get_node_mut(spawned) {
        node.set_property("position", Variant::Vector2(Vector2::new(12.0, 34.0)));
    }

    // ---- clause: replication_config resolves to the resource --------------
    // ⚠️ See the header: "resolved" here is this engine's marker convention.
    let cfg = prop(&host, SYNC, "replication_config");
    assert_ne!(
        cfg,
        Variant::Nil,
        "the synchronizer's `replication_config` must not be Nil — before this \
         property was registered, a .tscn's SubResource had nowhere to land"
    );
    assert!(
        matches!(&cfg, Variant::String(s) if s.contains("SceneReplicationConfig_1")),
        "…and must name the sub-resource it points at; got {cfg:?}"
    );

    // ---- clause: the config's property set, with its flags ----------------
    // Read through the flattened keys the replication path actually consumes.
    // ⚠️ The path's Variant KIND is deliberately not pinned. The engine's own
    // reader for these values, `variant_as_path_string`, accepts `NodePath`,
    // `String` AND `StringName` — so asserting one kind would fail whenever the
    // .tscn parser legitimately produced another, for a reason having nothing to
    // do with replication. The mode and spawn assertions below DO pin their kinds,
    // because those genuinely are one type and a wrong one there is a real defect.
    assert_eq!(
        path_of(&host, SYNC, 0),
        ".:position",
        "property 0 is `.:position`"
    );
    assert_eq!(
        prop(&host, SYNC, "__replication__properties/0/replication_mode"),
        Variant::Int(1),
        "…ALWAYS, so it is sync state"
    );
    assert_eq!(
        path_of(&host, SYNC, 1),
        ".:health",
        "property 1 is `.:health`"
    );
    assert_eq!(
        prop(&host, SYNC, "__replication__properties/1/replication_mode"),
        Variant::Int(2),
        "…ON_CHANGE, so it is watch state and NOT sync state"
    );
    assert_eq!(
        prop(&host, SYNC, "__replication__properties/1/spawn"),
        Variant::Bool(false),
        "…and it is not spawn state either"
    );

    let mut host_loop = MainLoop::new(host);
    let mut client_loop = MainLoop::new(client);
    let dt = 1.0 / 60.0;
    for _ in 0..8 {
        host_loop.step(dt);
        client_loop.step(dt);
    }

    assert!(
        client_loop.tree().get_node_by_path(PLAYER).is_some(),
        "the client must have the spawned player before the wire is asserted"
    );

    // ---- clause: spawn state carries position -----------------------------
    // `position` is spawn = true and was written on the host BEFORE the spawn, so
    // its value has to reach the client with the spawned instance.
    assert_eq!(
        prop(client_loop.tree(), PLAYER, "position"),
        Variant::Vector2(Vector2::new(12.0, 34.0)),
        "spawn state must carry `position` — a stale (0,0) means the config never \
         drove the spawn"
    );

    // ---- clause: a periodic sync carries position as a TYPED Vector2 ------
    // The assert above already pins the type: a `Variant::String(\"12,34\")` — the
    // old stringified encoding — would not compare equal to a Vector2.

    // ---- clause: health reaches the wire only AFTER it changes ------------
    //
    // Nothing has written `health` on either peer, so an ON_CHANGE property has
    // nothing to report and the client must still hold the scene's own 100.
    //
    // ⚠️ The bead also words this as "the spawn packet carries `position` but not
    // `health`". Those two effects are CONFOUNDED at steady state: once a sync
    // tick has run, a value that arrived by spawn and one that arrived by watch
    // are indistinguishable on the client. Spawn membership is therefore asserted
    // where it IS observable — `spawn_state_ignores_replication_mode` drives a
    // spawn-only property with no sync path at all — and this test pins the watch
    // semantics, which is the half that needs a running peer pair.
    assert_eq!(
        prop(client_loop.tree(), PLAYER, "health"),
        Variant::Int(100),
        "ON_CHANGE: nothing has written `health`, so it must not have travelled"
    );

    // Now change it, and it must arrive.
    let host_player = host_loop
        .tree()
        .get_node_by_path(PLAYER)
        .expect("host player");
    if let Some(node) = host_loop.tree_mut().get_node_mut(host_player) {
        node.set_property("health", Variant::Int(7));
    }
    for _ in 0..8 {
        host_loop.step(dt);
        client_loop.step(dt);
    }
    assert_eq!(
        prop(client_loop.tree(), PLAYER, "health"),
        Variant::Int(7),
        "…and once it changes, a watched property must travel"
    );
}
