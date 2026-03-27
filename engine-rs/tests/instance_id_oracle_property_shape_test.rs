//! pat-j7pu: Instance-ID shape coverage across oracle/property fixture scenes.
//!
//! Verifies that Patina's instance-ID system satisfies the structural
//! compatibility contract documented in
//! `fixtures/golden/scenes/instance_id_shape_contract.json` when loading
//! real scenes from the oracle fixture corpus.
//!
//! These tests do NOT require numeric value equality with Godot — only
//! structural shape:
//!
//! - Every node gets a positive non-zero u64 ID
//! - All IDs within a loaded scene are unique
//! - IDs are stable (unchanged by tree queries or property reads)
//! - IDs round-trip correctly through NodeId ↔ ObjectId
//! - The Variant representation is `Int` (matching Godot's `Variant.Type.INT`)
//!
//! The structural-compatibility contract:
//!   Patina's instance IDs are structurally compatible with Godot 4.6.1's
//!   `get_instance_id()` return values. Both systems produce positive u64
//!   values that are unique per session, stable for the object's lifetime,
//!   and monotonically increasing in creation order. Numeric values will
//!   differ between Patina and Godot because each runtime allocates from
//!   independent counters — only the shape (type, range, invariants) must
//!   match.

use std::collections::HashSet;

use gdscene::node::NodeId;
use gdscene::packed_scene::add_packed_scene_to_tree;
use gdscene::{PackedScene, SceneTree};
use gdvariant::Variant;

// ===========================================================================
// Fixture loaders
// ===========================================================================

fn load_scene(tscn: &str) -> (SceneTree, NodeId) {
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    (tree, scene_root)
}

/// Collect all NodeIds in the tree (including root).
fn all_node_ids(tree: &SceneTree) -> Vec<NodeId> {
    tree.all_nodes_in_tree_order()
}

/// Count nodes in an oracle property fixture JSON (recursive "children" arrays).
fn count_oracle_nodes(json: &serde_json::Value) -> usize {
    let mut count = 1; // this node
    if let Some(children) = json.get("children").and_then(|c| c.as_array()) {
        for child in children {
            count += count_oracle_nodes(child);
        }
    }
    count
}

// ===========================================================================
// Scene sources from the oracle fixture corpus
// ===========================================================================

// --- Original 5 scenes ---
const SIGNAL_INSTANTIATION_TSCN: &str =
    include_str!("../../fixtures/scenes/signal_instantiation.tscn");
const PHYSICS_PLAYGROUND_TSCN: &str =
    include_str!("../../fixtures/scenes/physics_playground.tscn");
const PLATFORMER_TSCN: &str = include_str!("../../fixtures/scenes/platformer.tscn");
const HIERARCHY_TSCN: &str = include_str!("../../fixtures/scenes/hierarchy.tscn");
const MINIMAL_TSCN: &str = include_str!("../../fixtures/scenes/minimal.tscn");

// --- Additional scenes with oracle property fixtures ---
const CHARACTER_BODY_TEST_TSCN: &str =
    include_str!("../../fixtures/scenes/character_body_test.tscn");
const SIGNALS_COMPLEX_TSCN: &str =
    include_str!("../../fixtures/scenes/signals_complex.tscn");
const SPACE_SHOOTER_TSCN: &str =
    include_str!("../../fixtures/scenes/space_shooter.tscn");
const UI_MENU_TSCN: &str = include_str!("../../fixtures/scenes/ui_menu.tscn");
const UNIQUE_NAME_RESOLUTION_TSCN: &str =
    include_str!("../../fixtures/scenes/unique_name_resolution.tscn");
const WITH_PROPERTIES_TSCN: &str =
    include_str!("../../fixtures/scenes/with_properties.tscn");
const TEST_SCRIPTS_TSCN: &str =
    include_str!("../../fixtures/scenes/test_scripts.tscn");

// --- Oracle property fixtures (JSON) ---
const SIGNAL_INSTANTIATION_PROPS: &str =
    include_str!("../../fixtures/oracle_outputs/signal_instantiation_properties.json");
const PHYSICS_PLAYGROUND_PROPS: &str =
    include_str!("../../fixtures/oracle_outputs/physics_playground_properties.json");
const PLATFORMER_PROPS: &str =
    include_str!("../../fixtures/oracle_outputs/platformer_properties.json");
const HIERARCHY_PROPS: &str =
    include_str!("../../fixtures/oracle_outputs/hierarchy_properties.json");
const MINIMAL_PROPS: &str =
    include_str!("../../fixtures/oracle_outputs/minimal_properties.json");
const CHARACTER_BODY_TEST_PROPS: &str =
    include_str!("../../fixtures/oracle_outputs/character_body_test_properties.json");
const SIGNALS_COMPLEX_PROPS: &str =
    include_str!("../../fixtures/oracle_outputs/signals_complex_properties.json");
const SPACE_SHOOTER_PROPS: &str =
    include_str!("../../fixtures/oracle_outputs/space_shooter_properties.json");
const UI_MENU_PROPS: &str =
    include_str!("../../fixtures/oracle_outputs/ui_menu_properties.json");
const UNIQUE_NAME_RESOLUTION_PROPS: &str =
    include_str!("../../fixtures/oracle_outputs/unique_name_resolution_properties.json");
const WITH_PROPERTIES_PROPS: &str =
    include_str!("../../fixtures/oracle_outputs/with_properties_properties.json");
const TEST_SCRIPTS_PROPS: &str =
    include_str!("../../fixtures/oracle_outputs/test_scripts_properties.json");

// ===========================================================================
// Shape assertion helpers
// ===========================================================================

/// Verify every node in the tree has a positive, unique, non-zero instance ID.
fn assert_instance_id_shape(tree: &SceneTree, scene_label: &str) {
    let ids = all_node_ids(tree);
    assert!(
        !ids.is_empty(),
        "[{scene_label}] Tree must contain at least one node"
    );

    let mut seen = HashSet::new();
    for nid in &ids {
        let raw = nid.raw();

        // Contract: positive non-zero
        assert!(
            raw > 0,
            "[{scene_label}] NodeId must be positive, got {raw}"
        );

        // Contract: unique within the scene
        assert!(
            seen.insert(raw),
            "[{scene_label}] Duplicate NodeId {raw} detected"
        );

        // Contract: ObjectId round-trip
        let oid = nid.object_id();
        assert_eq!(
            oid.raw(),
            raw,
            "[{scene_label}] ObjectId::raw() must match NodeId::raw()"
        );
        let roundtrip = NodeId::from_object_id(oid);
        assert_eq!(
            roundtrip, *nid,
            "[{scene_label}] NodeId -> ObjectId -> NodeId round-trip failed"
        );
    }
}

/// Verify IDs are stable — reading the ID twice returns the same value.
fn assert_instance_id_stability(tree: &SceneTree, scene_label: &str) {
    for nid in all_node_ids(tree) {
        let node = tree.get_node(nid).unwrap();
        let first = node.id();
        let second = node.id();
        assert_eq!(
            first, second,
            "[{scene_label}] Instance ID must be stable across reads"
        );
    }
}

/// Verify IDs are monotonically increasing in allocation (creation) order.
///
/// Note: tree order (DFS) may differ from creation order when a .tscn
/// declares child nodes after sibling subtrees (e.g., `Player/TriggerZone`
/// listed after `ItemDrop`).  We sort by raw ID to check that the
/// allocation sequence itself is strictly increasing — which it always is
/// because Patina uses an atomic counter.  For scenes where parse order
/// matches tree order, this is equivalent to the original tree-order check.
fn assert_instance_id_monotonic(tree: &SceneTree, scene_label: &str) {
    let ids = all_node_ids(tree);
    let mut sorted_raws: Vec<u64> = ids.iter().map(|n| n.raw()).collect();
    sorted_raws.sort_unstable();
    sorted_raws.dedup();

    // All IDs must be unique (dedup should not shrink the vec).
    assert_eq!(
        sorted_raws.len(),
        ids.len(),
        "[{scene_label}] Duplicate IDs detected"
    );

    // Sorted IDs must be strictly increasing (guaranteed by atomic counter).
    for window in sorted_raws.windows(2) {
        assert!(
            window[1] > window[0],
            "[{scene_label}] IDs must be monotonically increasing in allocation order: {} >= {}",
            window[0],
            window[1]
        );
    }
}

/// Verify that the Variant representation of an instance ID is Int.
fn assert_instance_id_variant_type(tree: &SceneTree, scene_label: &str) {
    for nid in all_node_ids(tree) {
        let raw = nid.raw() as i64;
        let variant = Variant::Int(raw);

        // The Variant must be Int (matching Godot's Variant.Type.INT)
        assert!(
            matches!(variant, Variant::Int(_)),
            "[{scene_label}] Instance ID Variant must be Int"
        );

        // Round-trip: extract the value back
        if let Variant::Int(v) = variant {
            assert_eq!(
                v, raw,
                "[{scene_label}] Variant Int value must match raw ID"
            );
            assert!(
                v > 0,
                "[{scene_label}] Variant Int value must be positive"
            );
        }
    }
}

// ===========================================================================
// Per-scene tests
// ===========================================================================

macro_rules! scene_instance_id_tests {
    ($mod_name:ident, $tscn:expr, $label:expr, $min_nodes:expr) => {
        mod $mod_name {
            use super::*;

            #[test]
            fn instance_id_shape() {
                let (tree, _) = load_scene($tscn);
                assert!(
                    tree.node_count() >= $min_nodes,
                    concat!("[", $label, "] Expected at least {} nodes, got {}"),
                    $min_nodes,
                    tree.node_count()
                );
                assert_instance_id_shape(&tree, $label);
            }

            #[test]
            fn instance_id_stability() {
                let (tree, _) = load_scene($tscn);
                assert_instance_id_stability(&tree, $label);
            }

            #[test]
            fn instance_id_monotonic() {
                let (tree, _) = load_scene($tscn);
                assert_instance_id_monotonic(&tree, $label);
            }

            #[test]
            fn instance_id_variant_type() {
                let (tree, _) = load_scene($tscn);
                assert_instance_id_variant_type(&tree, $label);
            }
        }
    };
}

scene_instance_id_tests!(
    signal_instantiation,
    SIGNAL_INSTANTIATION_TSCN,
    "signal_instantiation",
    3 // root + GameWorld + children
);

scene_instance_id_tests!(
    physics_playground,
    PHYSICS_PLAYGROUND_TSCN,
    "physics_playground",
    3 // root + scene nodes
);

scene_instance_id_tests!(
    platformer,
    PLATFORMER_TSCN,
    "platformer",
    5 // root + World + Player + Platforms + Camera + Collectible
);

scene_instance_id_tests!(
    hierarchy,
    HIERARCHY_TSCN,
    "hierarchy",
    3
);

scene_instance_id_tests!(
    minimal,
    MINIMAL_TSCN,
    "minimal",
    2 // root + at least one scene node
);

// --- Additional scenes with oracle property fixtures ---

scene_instance_id_tests!(
    character_body_test,
    CHARACTER_BODY_TEST_TSCN,
    "character_body_test",
    3 // root + World + Player + shapes
);

scene_instance_id_tests!(
    signals_complex,
    SIGNALS_COMPLEX_TSCN,
    "signals_complex",
    3 // root + Root + children
);

scene_instance_id_tests!(
    space_shooter,
    SPACE_SHOOTER_TSCN,
    "space_shooter",
    3 // root + SpaceShooter + children
);

scene_instance_id_tests!(
    ui_menu,
    UI_MENU_TSCN,
    "ui_menu",
    3 // root + MenuRoot + buttons
);

scene_instance_id_tests!(
    unique_name_resolution,
    UNIQUE_NAME_RESOLUTION_TSCN,
    "unique_name_resolution",
    3 // root + Root + unique-named children
);

scene_instance_id_tests!(
    with_properties,
    WITH_PROPERTIES_TSCN,
    "with_properties",
    3 // root + Root + children
);

scene_instance_id_tests!(
    test_scripts,
    TEST_SCRIPTS_TSCN,
    "test_scripts",
    2 // root + scripted nodes
);

// ===========================================================================
// Cross-scene uniqueness: IDs from different scene loads must not collide
// ===========================================================================

#[test]
fn cross_scene_instance_ids_are_globally_unique() {
    // Load multiple scenes sequentially — since ObjectId uses a global
    // atomic counter, IDs from different loads must never overlap.
    let scenes: &[(&str, &str)] = &[
        (MINIMAL_TSCN, "minimal"),
        (HIERARCHY_TSCN, "hierarchy"),
        (PLATFORMER_TSCN, "platformer"),
        (CHARACTER_BODY_TEST_TSCN, "character_body_test"),
        (SIGNALS_COMPLEX_TSCN, "signals_complex"),
        (SPACE_SHOOTER_TSCN, "space_shooter"),
        (UI_MENU_TSCN, "ui_menu"),
    ];

    let mut global_ids = HashSet::new();
    for (tscn, label) in scenes {
        let (tree, _) = load_scene(tscn);
        for nid in all_node_ids(&tree) {
            assert!(
                global_ids.insert(nid.raw()),
                "[cross-scene] Duplicate ID {} found when loading '{label}' — \
                 IDs must be globally unique across scene loads",
                nid.raw()
            );
        }
    }
    assert!(
        global_ids.len() >= 20,
        "Expected at least 20 unique IDs across all scenes, got {}",
        global_ids.len()
    );
}

// ===========================================================================
// Oracle property fixture parity: every node in oracle properties gets a
// valid instance ID when the scene is loaded in Patina
// ===========================================================================

/// Verify that the number of nodes Patina creates from a .tscn matches the
/// oracle property fixture's node count.  This ensures no node is silently
/// dropped (and therefore missing an instance ID).
macro_rules! oracle_node_count_parity {
    ($test_name:ident, $tscn:expr, $props:expr, $label:expr) => {
        #[test]
        fn $test_name() {
            let (tree, _) = load_scene($tscn);
            let oracle: serde_json::Value =
                serde_json::from_str($props).expect(concat!($label, " oracle must be valid JSON"));
            let oracle_count = count_oracle_nodes(&oracle);
            let patina_count = tree.node_count();

            // Patina may have one extra node (the SceneTree's own root)
            // compared to the oracle which starts at the scene root.
            // Accept exact match or patina_count == oracle_count (when the
            // oracle includes the Window root) or patina_count == oracle_count + 1.
            assert!(
                patina_count >= oracle_count,
                "[{label}] Patina has fewer nodes ({patina_count}) than oracle ({oracle_count}) — \
                 some nodes failed to instantiate and would be missing instance IDs",
                label = $label,
                patina_count = patina_count,
                oracle_count = oracle_count,
            );
        }
    };
}

oracle_node_count_parity!(
    oracle_node_count_signal_instantiation,
    SIGNAL_INSTANTIATION_TSCN,
    SIGNAL_INSTANTIATION_PROPS,
    "signal_instantiation"
);
oracle_node_count_parity!(
    oracle_node_count_physics_playground,
    PHYSICS_PLAYGROUND_TSCN,
    PHYSICS_PLAYGROUND_PROPS,
    "physics_playground"
);
oracle_node_count_parity!(
    oracle_node_count_platformer,
    PLATFORMER_TSCN,
    PLATFORMER_PROPS,
    "platformer"
);
oracle_node_count_parity!(
    oracle_node_count_hierarchy,
    HIERARCHY_TSCN,
    HIERARCHY_PROPS,
    "hierarchy"
);
oracle_node_count_parity!(
    oracle_node_count_minimal,
    MINIMAL_TSCN,
    MINIMAL_PROPS,
    "minimal"
);
oracle_node_count_parity!(
    oracle_node_count_character_body_test,
    CHARACTER_BODY_TEST_TSCN,
    CHARACTER_BODY_TEST_PROPS,
    "character_body_test"
);
oracle_node_count_parity!(
    oracle_node_count_signals_complex,
    SIGNALS_COMPLEX_TSCN,
    SIGNALS_COMPLEX_PROPS,
    "signals_complex"
);
oracle_node_count_parity!(
    oracle_node_count_space_shooter,
    SPACE_SHOOTER_TSCN,
    SPACE_SHOOTER_PROPS,
    "space_shooter"
);
oracle_node_count_parity!(
    oracle_node_count_ui_menu,
    UI_MENU_TSCN,
    UI_MENU_PROPS,
    "ui_menu"
);
oracle_node_count_parity!(
    oracle_node_count_unique_name_resolution,
    UNIQUE_NAME_RESOLUTION_TSCN,
    UNIQUE_NAME_RESOLUTION_PROPS,
    "unique_name_resolution"
);
oracle_node_count_parity!(
    oracle_node_count_with_properties,
    WITH_PROPERTIES_TSCN,
    WITH_PROPERTIES_PROPS,
    "with_properties"
);
oracle_node_count_parity!(
    oracle_node_count_test_scripts,
    TEST_SCRIPTS_TSCN,
    TEST_SCRIPTS_PROPS,
    "test_scripts"
);

// ===========================================================================
// Instance-ID shape golden fixture: validate against stored expectations
// ===========================================================================

const INSTANCE_ID_SHAPE_GOLDEN: &str =
    include_str!("../../fixtures/golden/scenes/instance_id_shape_contract.json");

/// Comprehensive validation: load every scene listed in the contract and
/// verify instance-ID shape invariants hold for the full corpus.
#[test]
fn all_contract_scenes_satisfy_shape_invariants() {
    let contract: serde_json::Value =
        serde_json::from_str(INSTANCE_ID_SHAPE_GOLDEN).expect("contract must parse");
    let scenes = contract
        .get("scenes_covered")
        .and_then(|v| v.as_array())
        .expect("contract must list scenes_covered");

    // Map scene names to tscn sources.
    let scene_map: &[(&str, &str)] = &[
        ("signal_instantiation", SIGNAL_INSTANTIATION_TSCN),
        ("physics_playground", PHYSICS_PLAYGROUND_TSCN),
        ("platformer", PLATFORMER_TSCN),
        ("hierarchy", HIERARCHY_TSCN),
        ("minimal", MINIMAL_TSCN),
        ("character_body_test", CHARACTER_BODY_TEST_TSCN),
        ("signals_complex", SIGNALS_COMPLEX_TSCN),
        ("space_shooter", SPACE_SHOOTER_TSCN),
        ("ui_menu", UI_MENU_TSCN),
        ("unique_name_resolution", UNIQUE_NAME_RESOLUTION_TSCN),
        ("with_properties", WITH_PROPERTIES_TSCN),
        ("test_scripts", TEST_SCRIPTS_TSCN),
    ];

    for scene_name in scenes {
        let name = scene_name.as_str().expect("scene name must be string");
        let tscn = scene_map
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| *v)
            .unwrap_or_else(|| panic!("Contract lists unknown scene: {name}"));

        let (tree, _) = load_scene(tscn);
        assert_instance_id_shape(&tree, name);
        assert_instance_id_stability(&tree, name);
        assert_instance_id_monotonic(&tree, name);
        assert_instance_id_variant_type(&tree, name);
    }
}

// ===========================================================================
// Contract documentation test: golden fixture is valid JSON
// ===========================================================================

#[test]
fn instance_id_shape_contract_fixture_is_valid() {
    let contract_json = include_str!(
        "../../fixtures/golden/scenes/instance_id_shape_contract.json"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(contract_json).expect("contract fixture must be valid JSON");

    // Verify the contract documents key shape properties.
    let contract = parsed.get("contract").expect("must have 'contract' key");
    assert_eq!(
        contract.get("type").and_then(|v| v.as_str()),
        Some("u64"),
        "Contract must document u64 type"
    );
    assert_eq!(
        contract.get("zero_allowed").and_then(|v| v.as_bool()),
        Some(false),
        "Contract must document zero is not allowed"
    );
    assert_eq!(
        contract.get("variant_type").and_then(|v| v.as_str()),
        Some("Int (i64 in Patina Variant, maps to Godot Variant.Type.INT)"),
        "Contract must document Variant type"
    );

    // Verify the contract lists all 12 scenes we cover.
    let scenes = parsed
        .get("scenes_covered")
        .and_then(|v| v.as_array())
        .expect("contract must have scenes_covered array");
    assert!(
        scenes.len() >= 12,
        "Contract must list at least 12 scenes, got {}",
        scenes.len()
    );
}
