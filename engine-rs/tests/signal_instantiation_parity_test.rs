//! pat-o2k3: Oracle-backed parity test for scene-level signal connections
//! wired during instantiation.
//!
//! Observable behavior checked:
//! - All `[connection]` entries in a `.tscn` file produce live signal
//!   connections on the correct source node immediately after
//!   `add_packed_scene_to_tree`.
//! - Each connection's signal name, target method, and one-shot flag match
//!   the oracle golden (`signal_instantiation_connections.json`).
//! - Multi-connection signals (two listeners on the same signal) are both
//!   present.
//! - Cross-hierarchy wiring (nested child → sibling, root → deep child)
//!   resolves correctly.

mod oracle_fixture;

use oracle_fixture::fixtures_dir;
use serde_json::Value;

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

// ---------------------------------------------------------------------------
// Oracle golden
// ---------------------------------------------------------------------------

/// One expected connection from the oracle golden file.
#[derive(Debug)]
struct OracleConnection {
    signal_name: String,
    from_node: String,
    to_node: String,
    method: String,
    flags: u32,
}

fn load_oracle_connections() -> Vec<OracleConnection> {
    let path = fixtures_dir()
        .join("oracle_outputs")
        .join("signal_instantiation_connections.json");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to load oracle connections: {e}"));
    let root: Value = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse oracle connections: {e}"));

    root["connections"]
        .as_array()
        .expect("oracle connections must be an array")
        .iter()
        .map(|c| OracleConnection {
            signal_name: c["signal_name"].as_str().unwrap().to_owned(),
            from_node: c["from_node"].as_str().unwrap().to_owned(),
            to_node: c["to_node"].as_str().unwrap().to_owned(),
            method: c["method"].as_str().unwrap().to_owned(),
            flags: c["flags"].as_u64().unwrap() as u32,
        })
        .collect()
}

fn load_signal_instantiation_tscn() -> String {
    let path = fixtures_dir()
        .join("scenes")
        .join("signal_instantiation.tscn");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to load signal_instantiation.tscn: {e}"))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve a relative connection path (from the .tscn) to an absolute tree
/// path under the scene root. "." means the scene root itself.
fn resolve_connection_path(scene_root_path: &str, relative: &str) -> String {
    if relative == "." {
        scene_root_path.to_owned()
    } else {
        format!("{scene_root_path}/{relative}")
    }
}

/// A live connection extracted from the tree after instantiation.
struct LiveConnection {
    signal_name: String,
    method: String,
    one_shot: bool,
    target_path: String,
}

/// Walk every signal store in the tree and collect all connections with
/// resolved target paths.
fn extract_live_connections(tree: &SceneTree) -> Vec<LiveConnection> {
    // Build ObjectId → path map from all nodes that have signal stores.
    // We also need target nodes that may not have stores, so build the map
    // from the signal store keys + all target ObjectIds we encounter.
    let stores = tree.signal_stores();

    // First pass: collect all NodeIds from signal stores.
    let store_node_ids: Vec<_> = stores.keys().copied().collect();

    // Build a NodeId → path map for source nodes.
    let mut oid_to_path = std::collections::HashMap::new();
    for &nid in &store_node_ids {
        if let Some(path) = tree.node_path(nid) {
            oid_to_path.insert(nid.object_id(), path);
        }
    }

    // Collect all target ObjectIds so we can resolve their paths too.
    // We resolve target paths by scanning stores for target_ids, then
    // looking up the node by checking all known nodes.
    let mut result = Vec::new();
    for (&nid, store) in stores {
        for sig_name in store.signal_names() {
            if let Some(sig) = store.get_signal(sig_name) {
                for conn in sig.connections() {
                    // Resolve target path: try the oid_to_path map first,
                    // otherwise try to find the node via the tree.
                    let target_path = if let Some(p) = oid_to_path.get(&conn.target_id) {
                        p.clone()
                    } else {
                        // The target node may not have a signal store itself.
                        // We need to resolve its ObjectId → NodeId → path.
                        // Use a helper: scan the connection's target_id.
                        resolve_object_id_path(tree, conn.target_id)
                    };
                    result.push(LiveConnection {
                        signal_name: sig_name.to_owned(),
                        method: conn.method.clone(),
                        one_shot: conn.one_shot,
                        target_path,
                    });
                }
            }
        }
        // Also add the source node to oid_to_path if not already there.
        if let Some(path) = tree.node_path(nid) {
            oid_to_path.insert(nid.object_id(), path);
        }
    }
    result
}

/// Resolve an ObjectId to a node path by checking well-known paths in the
/// scene. This is used when the target node doesn't have its own signal store.
fn resolve_object_id_path(tree: &SceneTree, oid: gdcore::id::ObjectId) -> String {
    // Try all paths we expect in the test scene.
    let known_paths = [
        "/root/GameWorld",
        "/root/GameWorld/Player",
        "/root/GameWorld/Player/Hitbox",
        "/root/GameWorld/Enemy",
        "/root/GameWorld/HUD",
        "/root/GameWorld/HUD/ScoreLabel",
    ];
    for path in &known_paths {
        if let Some(nid) = tree.get_node_by_path(path) {
            if nid.object_id() == oid {
                return path.to_string();
            }
        }
    }
    format!("<unresolved:{:?}>", oid)
}

// ===========================================================================
// Tests
// ===========================================================================

/// Core parity test: every oracle connection must exist in the live tree after
/// instantiation, with matching signal name, target, method, and flags.
#[test]
fn signal_connections_match_oracle_after_instantiation() {
    let tscn = load_signal_instantiation_tscn();
    let scene = PackedScene::from_tscn(&tscn).expect("parse signal_instantiation.tscn");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _scene_root = add_packed_scene_to_tree(&mut tree, root, &scene)
        .expect("add_packed_scene_to_tree");

    let oracle = load_oracle_connections();
    let live = extract_live_connections(&tree);

    let scene_root_path = "/root/GameWorld";

    let mut missing: Vec<String> = Vec::new();

    for oc in &oracle {
        let expected_target = resolve_connection_path(scene_root_path, &oc.to_node);
        let expected_one_shot = oc.flags & 4 != 0; // Godot CONNECT_ONE_SHOT = bit 2 (value 4)

        let found = live.iter().any(|lc| {
            lc.signal_name == oc.signal_name
                && lc.method == oc.method
                && lc.target_path == expected_target
                && lc.one_shot == expected_one_shot
        });

        if !found {
            missing.push(format!(
                "  signal={} from={} to={} method={} one_shot={}",
                oc.signal_name, oc.from_node, oc.to_node, oc.method, expected_one_shot,
            ));
        }
    }

    assert!(
        missing.is_empty(),
        "Oracle connections NOT found after instantiation:\n{}",
        missing.join("\n")
    );
}

/// The total number of live connections must equal the oracle count — no
/// phantom extras.
#[test]
fn no_extra_connections_beyond_oracle() {
    let tscn = load_signal_instantiation_tscn();
    let scene = PackedScene::from_tscn(&tscn).expect("parse");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("instantiate");

    let oracle_count = load_oracle_connections().len();
    let live_count = extract_live_connections(&tree).len();

    assert_eq!(
        live_count, oracle_count,
        "Expected {oracle_count} connections (from oracle), found {live_count} in live tree"
    );
}

/// Multi-connection: Player's "health_changed" signal must have exactly 2
/// connections (HUD + ScoreLabel).
#[test]
fn multi_connection_same_signal() {
    let tscn = load_signal_instantiation_tscn();
    let scene = PackedScene::from_tscn(&tscn).expect("parse");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instantiate");

    let player_id = tree
        .get_node_by_path("/root/GameWorld/Player")
        .expect("Player node must exist");
    let store = tree
        .signal_store(player_id)
        .expect("Player must have a signal store");
    let health = store
        .get_signal("health_changed")
        .expect("Player must have 'health_changed' signal");

    assert_eq!(
        health.connection_count(),
        2,
        "health_changed should have 2 connections (HUD and ScoreLabel), found {}",
        health.connection_count()
    );
}

/// One-shot flag: Enemy's "defeated" connection (flags=3) must be marked
/// one_shot.
#[test]
fn one_shot_flag_from_tscn_flags() {
    let tscn = load_signal_instantiation_tscn();
    let scene = PackedScene::from_tscn(&tscn).expect("parse");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instantiate");

    let enemy_id = tree
        .get_node_by_path("/root/GameWorld/Enemy")
        .expect("Enemy node must exist");
    let store = tree
        .signal_store(enemy_id)
        .expect("Enemy must have a signal store");
    let defeated = store
        .get_signal("defeated")
        .expect("Enemy must have 'defeated' signal");

    assert_eq!(defeated.connection_count(), 1);
    assert!(
        defeated.connections()[0].one_shot,
        "defeated connection should be one-shot (flags=3)"
    );
}

/// Nested child to sibling: Player/Hitbox → Enemy connection resolves across
/// hierarchy.
#[test]
fn nested_child_to_sibling_connection() {
    let tscn = load_signal_instantiation_tscn();
    let scene = PackedScene::from_tscn(&tscn).expect("parse");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instantiate");

    let hitbox_id = tree
        .get_node_by_path("/root/GameWorld/Player/Hitbox")
        .expect("Hitbox node must exist");
    let store = tree
        .signal_store(hitbox_id)
        .expect("Hitbox must have a signal store");
    let hit = store
        .get_signal("hit")
        .expect("Hitbox must have 'hit' signal");

    assert_eq!(hit.connection_count(), 1);
    assert_eq!(hit.connections()[0].method, "_on_hitbox_hit");

    // Verify the target is the Enemy node.
    let enemy_id = tree
        .get_node_by_path("/root/GameWorld/Enemy")
        .expect("Enemy must exist");
    assert_eq!(
        hit.connections()[0].target_id,
        enemy_id.object_id(),
        "hit connection target should be Enemy"
    );
}

/// Root-to-deep-child: GameWorld's "score_updated" connects to HUD/ScoreLabel.
#[test]
fn root_to_deep_child_connection() {
    let tscn = load_signal_instantiation_tscn();
    let scene = PackedScene::from_tscn(&tscn).expect("parse");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("instantiate");

    let store = tree
        .signal_store(scene_root)
        .expect("GameWorld must have a signal store");
    let score = store
        .get_signal("score_updated")
        .expect("GameWorld must have 'score_updated' signal");

    assert_eq!(score.connection_count(), 1);
    assert_eq!(score.connections()[0].method, "_on_score_updated");

    let score_label_id = tree
        .get_node_by_path("/root/GameWorld/HUD/ScoreLabel")
        .expect("ScoreLabel must exist");
    assert_eq!(
        score.connections()[0].target_id,
        score_label_id.object_id(),
        "score_updated target should be HUD/ScoreLabel"
    );
}

/// Child-to-root: Player's "died" signal connects to "." (GameWorld root).
#[test]
fn child_to_scene_root_connection() {
    let tscn = load_signal_instantiation_tscn();
    let scene = PackedScene::from_tscn(&tscn).expect("parse");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("instantiate");

    let player_id = tree
        .get_node_by_path("/root/GameWorld/Player")
        .expect("Player must exist");
    let store = tree
        .signal_store(player_id)
        .expect("Player must have a signal store");
    let died = store
        .get_signal("died")
        .expect("Player must have 'died' signal");

    assert_eq!(died.connection_count(), 1);
    assert_eq!(died.connections()[0].method, "_on_player_died");
    assert_eq!(
        died.connections()[0].target_id,
        scene_root.object_id(),
        "died target should be scene root (GameWorld)"
    );
}
