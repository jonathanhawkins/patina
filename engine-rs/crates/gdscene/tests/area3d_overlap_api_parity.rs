//! pat-s3rrv.102 — `Area3D` publishes its area-vs-area overlap set, so
//! `get_overlapping_areas()` / `has_overlapping_areas()` / `overlaps_area()`
//! answer truthfully from GDScript.
//!
//! Godot ground truth: `doc/classes/Area3D.xml` —
//! `get_overlapping_areas()`, `has_overlapping_areas()`, `overlaps_area()`
//! alongside the body variants; `upstream/godot/scene/3d/physics/area_3d.cpp`.
//!
//! ⚠️ **The bug this covers was a disagreement between two working halves.**
//! `gdscene::scripting` answers all three methods from the node property
//! `_overlapping_areas`, and those arms are class-agnostic — they serve Area2D
//! and Area3D alike. But only the **2D** server ever wrote that property
//! (`PhysicsServer::publish_overlapping_areas`); the 3D server published
//! `_overlapping_bodies` and nothing else. So an `Area3D` fired `area_entered`
//! correctly while `get_overlapping_areas()` returned an empty Array forever.
//! Signals and queries contradicting each other reads as a script bug, which is
//! why the gap survived: every individual piece worked.
//!
//! ⚠️ **SCOPE.** Clauses (a) and (b) of the bead's acceptance. Clause (c) — the
//! four shape-indexed signals (`body_shape_entered` and friends) — is NOT
//! implemented and NOT tested: `OverlapEvent3D` / `AreaOverlapEvent3D` carry no
//! shape index, so it needs threading from the narrow phase before any such
//! signal can be emitted. Do not read a green run of this file as the whole
//! acceptance being met.
//!
//! The clause (b) coverage here is deliberately thin: it proves the authored
//! property reaches the physics layer. The MODE SEMANTICS (Combine sums,
//! Replace stops the chain, ReplaceCombine keeps walking, a zero Replace is an
//! override rather than an absence) are pinned in `gdphysics3d::world`'s own
//! tests, where they can be asserted exactly instead of inferred from a
//! velocity.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, GDScriptNodeInstance};
use gdvariant::Variant;

/// Reads the script-facing query from inside the running scene, so the test
/// exercises the same path a game does rather than the Rust API underneath it.
///
/// ⚠️ `seen` starts at -1, not 0. A script that never ran and a script that ran
/// and saw nothing are different outcomes, and 0 would conflate them — the
/// "separated" assertion below expects 0 and would pass vacuously against a
/// `_physics_process` that was never dispatched.
const ZONE_A_GD: &str = "\
extends Area3D
var seen = -1
var any = false
func _physics_process(delta):
    self.seen = get_overlapping_areas().size()
    self.any = has_overlapping_areas()
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// Adds the `CollisionShape3D` child that `shape_from_node_3d` reads.
fn add_box(tree: &mut SceneTree, parent: NodeId, size: f32) {
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
    shape.set_property("size", Variant::Vector3(Vector3::new(size, size, size)));
    tree.add_child(parent, shape).unwrap();
}

/// `Root -> {ZoneA, ZoneB}`, two 4×4×4 Area3D boxes. ZoneB starts 1m from ZoneA,
/// well inside it. Returns `(loop, zone_a, zone_b)`.
///
/// Both areas keep Godot's defaults for `monitoring` and `monitorable` (both
/// true), which is what makes the pair mutually visible.
fn two_areas() -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let zone_a = tree.add_child(root, Node::new("ZoneA", "Area3D")).unwrap();
    node3d::set_position(&mut tree, zone_a, Vector3::ZERO);
    add_box(&mut tree, zone_a, 4.0);

    let zone_b = tree.add_child(root, Node::new("ZoneB", "Area3D")).unwrap();
    node3d::set_position(&mut tree, zone_b, Vector3::new(1.0, 0.0, 0.0));
    add_box(&mut tree, zone_b, 4.0);

    tree.attach_script(
        zone_a,
        Box::new(GDScriptNodeInstance::from_source(ZONE_A_GD, zone_a).unwrap()),
    );

    (MainLoop::new(tree), zone_a, zone_b)
}

/// The published node property, which is the single source both
/// `get_overlapping_areas()` and `overlaps_area()` read.
fn published_overlaps(ml: &MainLoop, node: NodeId) -> Vec<Variant> {
    match ml
        .tree()
        .get_node(node)
        .map(|n| n.get_property("_overlapping_areas"))
    {
        Some(Variant::Array(items)) => items,
        Some(Variant::Nil) | None => Vec::new(),
        Some(other) => panic!("_overlapping_areas must be an Array, got {other:?}"),
    }
}

const STEP: f64 = 1.0 / 60.0;

/// **The acceptance test (clause a).** Two overlapping `Area3D`s see each other
/// from script, and stop seeing each other once separated.
#[test]
fn area3d_overlap_api_parity() {
    let (mut ml, zone_a, zone_b) = two_areas();

    // The 3D server registers lazily inside `sync_to_physics`, so the first tick
    // registers, detects and publishes; the extra ticks let the script observe.
    for _ in 0..3 {
        ml.step(STEP);
    }

    // (1) The publisher itself.
    let overlaps = published_overlaps(&ml, zone_a);
    assert_eq!(
        overlaps.len(),
        1,
        "ZoneA must publish exactly one overlapping area, got {overlaps:?}"
    );
    // ⚠️ Identity, not just arity — a publisher that wrote the area's OWN id, or
    // some other node, would satisfy a bare length check. `detect_area_overlaps`
    // records directed pairs, so ZoneA's entry must be ZoneB.
    let expected = zone_b.object_id();
    assert_eq!(
        overlaps[0],
        Variant::ObjectId(expected),
        "ZoneA's overlap entry must be ZoneB itself"
    );

    // (2) The script surface, which is what the bead is actually about.
    assert_eq!(
        script_var(&ml, zone_a, "seen"),
        Some(Variant::Int(1)),
        "get_overlapping_areas() must report one area from script; -1 means \
         _physics_process never ran, 0 means the property was never published"
    );
    assert_eq!(
        script_var(&ml, zone_a, "any"),
        Some(Variant::Bool(true)),
        "has_overlapping_areas() must be true while the areas overlap"
    );

    // (3) Separate them. Godot's answer is a live set, not a latch.
    node3d::set_position(ml.tree_mut(), zone_b, Vector3::new(100.0, 0.0, 0.0));
    for _ in 0..3 {
        ml.step(STEP);
    }

    assert!(
        published_overlaps(&ml, zone_a).is_empty(),
        "once separated, ZoneA must publish an empty overlap set"
    );
    assert_eq!(
        script_var(&ml, zone_a, "seen"),
        Some(Variant::Int(0)),
        "get_overlapping_areas() must be empty after the areas separate — a \
         non-zero value here means the set latches instead of tracking"
    );
    assert_eq!(
        script_var(&ml, zone_a, "any"),
        Some(Variant::Bool(false)),
        "has_overlapping_areas() must be false once separated"
    );
}

/// An area that opted out of being seen never appears in another area's answer.
///
/// ⚠️ This is the gate that makes the published set correct rather than merely
/// non-empty, and it is NOT re-implemented in the publisher: `monitorable` is
/// applied inside `detect_area_overlaps`, so the publisher reads an
/// already-filtered set. The test pins that, because a future publisher that
/// recomputed overlaps itself would be the obvious place to forget it.
#[test]
fn a_non_monitorable_area_is_not_published_as_overlapping() {
    let (mut ml, zone_a, zone_b) = two_areas();
    if let Some(node) = ml.tree_mut().get_node_mut(zone_b) {
        node.set_property("monitorable", Variant::Bool(false));
    }
    for _ in 0..3 {
        ml.step(STEP);
    }

    assert!(
        published_overlaps(&ml, zone_a).is_empty(),
        "a non-monitorable area must not appear in another area's overlap set"
    );
    assert_eq!(
        script_var(&ml, zone_a, "seen"),
        Some(Variant::Int(0)),
        "and the script must agree — 1 here means the monitorable gate was lost"
    );
}

/// **Clause (b), the bridge half.** An authored `linear_damp_space_override`
/// reaches `gdphysics3d::Area3D`.
///
/// ⚠️ Godot defaults both damp overrides to `0` (DISABLED) — verified against
/// `doc/classes/Area3D.xml`. Before this, the 3D bridge read only
/// `gravity_space_override`, and `PhysicsWorld3D` applied every overlapping
/// area's `linear_damp` unconditionally. Since `Area3D.linear_damp` itself
/// defaults to 0.1, that meant EVERY area silently damped whatever was inside
/// it, no matter what the scene authored.
#[test]
fn area3d_damp_space_override_reaches_the_physics_layer() {
    let (mut ml, zone_a, zone_b) = two_areas();
    if let Some(node) = ml.tree_mut().get_node_mut(zone_a) {
        // 1 == COMBINE.
        node.set_property("linear_damp_space_override", Variant::Int(1));
        node.set_property("linear_damp", Variant::Float(5.0));
    }
    ml.step(STEP);

    let area_id = ml
        .physics_server()
        .area_for_node(zone_a)
        .expect("ZoneA registers an area");
    let area = ml
        .physics_server()
        .world()
        .get_area(area_id)
        .expect("the area exists in the world");

    assert_eq!(
        area.linear_damp_space_override,
        gdphysics3d::area3d::SpaceOverride::Combine,
        "the authored COMBINE mode must reach the area"
    );
    assert!(
        (area.linear_damp - 5.0).abs() < 1e-6,
        "and so must the damp value, got {}",
        area.linear_damp
    );

    // ⚠️ Positive control on the DEFAULT: the untouched sibling must come back
    // DISABLED, not COMBINE. Without this the test would pass against a bridge
    // that hard-coded every area to COMBINE.
    let other_id = ml
        .physics_server()
        .area_for_node(zone_b)
        .expect("ZoneB registers an area");
    let other = ml
        .physics_server()
        .world()
        .get_area(other_id)
        .expect("ZoneB area exists");
    assert_eq!(
        other.linear_damp_space_override,
        gdphysics3d::area3d::SpaceOverride::Disabled,
        "an unauthored area must keep Godot's DISABLED default"
    );
}
