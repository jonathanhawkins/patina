//! pat-s3rrv.177 — `REPLICATION_MODE_ON_CHANGE` (Godot's *watch* properties).
//!
//! Godot ground truth: `modules/multiplayer/scene_replication_config.h` declares
//! `enum ReplicationMode { REPLICATION_MODE_NEVER, REPLICATION_MODE_ALWAYS,
//! REPLICATION_MODE_ON_CHANGE }` — transcribed positionally, so **NEVER 0,
//! ALWAYS 1, ON_CHANGE 2**. `_update()` in `scene_replication_config.cpp` routes
//! `prop.spawn` into `spawn_props`, mode ALWAYS into `sync_props` and mode
//! ON_CHANGE into `watch_props`.
//!
//! 🔑 **`sync` and `watch` are two views over ONE `mode` field**, not independent
//! flags: `property_get_watch()` is literally `mode == REPLICATION_MODE_ON_CHANGE`,
//! and `property_set_sync(p, true)` just sets `mode = ALWAYS`. A property is
//! therefore in exactly one of the two sets, never both.
//!
//! ⚠️ **The defect this pins.** The reader skipped only `replication_mode == 0`,
//! so ON_CHANGE fell through into the *sync* list and a watched property was
//! retransmitted **every tick** — precisely the traffic the mode exists to avoid.
//! There was no watch list at all.
//!
//! ⚠️ Testing absence needs a control, so each "must not travel" assertion below
//! is paired with an ALWAYS property put through the *identical* manipulation.
//! Without that pairing, a synchronizer that had simply stopped replicating would
//! pass every negative assertion here.

use gdcore::math::Vector2;
use gdplatform::network::{MockNetwork, PeerId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// `position` is ALWAYS (mode 1); `health` is ON_CHANGE (mode 2).
///
/// Both are `spawn = true`, so spawn state is identical for the two and cannot
/// explain any difference the tests observe.
const PLAYER_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="SceneReplicationConfig" id="SceneReplicationConfig_1"]
properties/0/path = NodePath(".:position")
properties/0/spawn = true
properties/0/replication_mode = 1
properties/1/path = NodePath(".:health")
properties/1/spawn = true
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

fn prop_at(tree: &SceneTree, path: &str, name: &str) -> Variant {
    match tree.get_node_by_path(path).and_then(|id| tree.get_node(id)) {
        Some(node) => node.get_property(name),
        None => Variant::Nil,
    }
}

const PLAYER: &str = "/root/Main/Players/Player";

/// A host/client pair with one replicated player already spawned on the client.
struct Session {
    host: MainLoop,
    client: MainLoop,
    _dir: tempfile::TempDir,
}

impl Session {
    fn start() -> Self {
        Self::start_with(PLAYER_TSCN)
    }

    fn start_with(player_tscn: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let player_path = dir.path().join("player.tscn");
        std::fs::write(&player_path, player_tscn).expect("write player scene");
        let player_path = player_path.to_string_lossy().to_string();

        let mut host = load_tree(&world_tscn(&player_path));
        let mut client = load_tree(&world_tscn(&player_path));

        let (server, client_peer) = MockNetwork::create_pair(PeerId(2));
        host.get_multiplayer_mut()
            .set_multiplayer_peer(Box::new(server));
        client
            .get_multiplayer_mut()
            .set_multiplayer_peer(Box::new(client_peer));

        let player_scene = PackedScene::from_tscn(player_tscn).expect("player scene parses");
        let players = host
            .get_node_by_path("/root/Main/Players")
            .expect("host has a Players container");
        add_packed_scene_to_tree(&mut host, players, &player_scene)
            .expect("host instances a player");

        let mut session = Self {
            host: MainLoop::new(host),
            client: MainLoop::new(client),
            _dir: dir,
        };
        session.pump(6);
        assert!(
            session.client.tree().get_node_by_path(PLAYER).is_some(),
            "precondition: the client must have the spawned player before any \
             assertion below measures replication"
        );
        session
    }

    fn pump(&mut self, frames: usize) {
        let dt = 1.0 / 60.0;
        for _ in 0..frames {
            self.host.step(dt);
            self.client.step(dt);
        }
    }

    fn set_host(&mut self, name: &str, value: Variant) {
        let id = self
            .host
            .tree()
            .get_node_by_path(PLAYER)
            .expect("host player");
        if let Some(node) = self.host.tree_mut().get_node_mut(id) {
            node.set_property(name, value);
        }
    }

    /// Overwrites the CLIENT's copy so a later retransmission is observable as
    /// the sentinel being replaced.
    fn poison_client(&mut self, name: &str, value: Variant) {
        let id = self
            .client
            .tree()
            .get_node_by_path(PLAYER)
            .expect("client player");
        if let Some(node) = self.client.tree_mut().get_node_mut(id) {
            node.set_property(name, value);
        }
    }

    fn client_prop(&self, name: &str) -> Variant {
        prop_at(self.client.tree(), PLAYER, name)
    }
}

/// A watched property reaches the client when its value changes.
#[test]
fn a_watched_property_travels_when_it_changes() {
    let mut s = Session::start();

    s.set_host("health", Variant::Int(42));
    s.pump(6);

    assert_eq!(
        s.client_prop("health"),
        Variant::Int(42),
        "`replication_mode = 2` is ON_CHANGE — a changed value must still \
         replicate. Never travelling would mean the watch list is read but \
         nothing ever sends it"
    );
}

/// 🔑 The headline: once sent, a watched property is NOT resent while unchanged.
///
/// ⚠️ The ALWAYS property goes through the identical poison-and-wait and MUST be
/// overwritten. Without that half, a synchronizer that had simply stopped
/// replicating would satisfy the `health` assertion.
#[test]
fn a_watched_property_is_not_retransmitted_while_unchanged() {
    let mut s = Session::start();

    // Let both properties reach the client once.
    s.set_host("health", Variant::Int(42));
    s.set_host("position", Variant::Vector2(Vector2::new(1.0, 2.0)));
    s.pump(6);
    assert_eq!(s.client_prop("health"), Variant::Int(42), "health arrived");

    // Poison BOTH client copies, then let frames pass without touching the host.
    s.poison_client("health", Variant::Int(-1));
    s.poison_client("position", Variant::Vector2(Vector2::new(-1.0, -1.0)));
    s.pump(8);

    assert_eq!(
        s.client_prop("health"),
        Variant::Int(-1),
        "ON_CHANGE: the host's `health` did not change, so nothing may be sent \
         and the sentinel must survive. Seeing 42 again means the watched \
         property is riding the every-tick sync list"
    );
    assert_eq!(
        s.client_prop("position"),
        Variant::Vector2(Vector2::new(1.0, 2.0)),
        "control: ALWAYS is retransmitted every tick, so the identical sentinel \
         MUST be overwritten. If this also survived, replication had simply \
         stopped and the assertion above proved nothing"
    );
}

/// …and a second change after the quiet period still travels.
///
/// ⚠️ Guards the "recorded as sent too early" bug: if the last-sent value were
/// updated when a change is *detected* rather than when a packet is actually
/// sent, an update that coincided with a blocked interval would be dropped
/// permanently and this would keep the sentinel.
#[test]
fn a_watched_property_travels_again_after_a_later_change() {
    let mut s = Session::start();

    s.set_host("health", Variant::Int(42));
    s.pump(6);
    s.poison_client("health", Variant::Int(-1));
    s.pump(4);
    assert_eq!(
        s.client_prop("health"),
        Variant::Int(-1),
        "precondition: quiet period sent nothing"
    );

    s.set_host("health", Variant::Int(7));
    s.pump(6);

    assert_eq!(
        s.client_prop("health"),
        Variant::Int(7),
        "a watched property must resume travelling on the NEXT change; staying \
         at the sentinel means its last-sent value was recorded without a send"
    );
}

/// A config with NO sync properties at all — every entry is ON_CHANGE.
///
/// ⚠️ The scene-side snapshot loop skipped any synchronizer whose *sync* list was
/// empty, so this shape was dropped before its values were ever read. The
/// mixed-mode config in the tests above cannot catch that: its sync list is
/// non-empty, so the guard passes for the wrong reason.
const WATCH_ONLY_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="SceneReplicationConfig" id="SceneReplicationConfig_1"]
properties/0/path = NodePath(".:health")
properties/0/spawn = true
properties/0/replication_mode = 2

[node name="Player" type="Node2D"]
position = Vector2(0, 0)
health = 100

[node name="Sync" type="MultiplayerSynchronizer" parent="."]
root_path = NodePath("..")
replication_interval = 0.0
replication_config = SubResource("SceneReplicationConfig_1")
"#;

#[test]
fn a_synchronizer_with_only_watched_properties_still_replicates() {
    let mut s = Session::start_with(WATCH_ONLY_TSCN);

    s.set_host("health", Variant::Int(55));
    s.pump(6);

    assert_eq!(
        s.client_prop("health"),
        Variant::Int(55),
        "a config whose every property is ON_CHANGE must still replicate; \
         staying at the scene's 100 means the synchronizer was skipped entirely \
         for having an empty sync list"
    );
}
