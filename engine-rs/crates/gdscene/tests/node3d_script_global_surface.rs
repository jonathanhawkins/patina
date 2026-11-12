//! pat-mgixk.30 acceptance: Node3D's global/basis/quaternion surface is
//! reachable from GDScript.
//!
//! # What was wrong
//!
//! Godot's Node3D exposes `global_position`, `global_transform`,
//! `global_basis`, `global_rotation(_degrees)`, `basis`, `quaternion` and
//! `rotation_degrees` as real properties, plus a method table of ~20 names
//! (`doc/classes/Node3D.xml`, `scene/3d/node_3d.cpp`).
//!
//! Patina stored only `transform`/`position`/`rotation`/`scale`, and
//! `get_node_property` fell through to the raw property bag — so
//! `$Node.global_position` on a Node3D read back `Nil`, the single most ordinary
//! thing a 3D script does. Only SEVEN methods were dispatched (`look_at`,
//! `translate`, `rotate`, `rotate_x..z`, `to_global`, `to_local`); ClassDB
//! registered the rest, so a call to any other returned `Nil` silently on
//! another node and `UndefinedFunction` on `self`.
//!
//! # The path driven
//!
//! ```text
//! .tscn -> PackedScene -> SceneTree -> attached GDScript
//!   -> `$Probe.global_position` (interpreter member access)
//!     -> SceneAccess::get_node_property -> node3d::get_derived_property
//! ```
//!
//! and for the writes, `$Probe.global_transform = t` ->
//! `SceneAccess::set_node_property` -> `node3d::set_derived_property`.
//!
//! ⚠️ The assertions deliberately read BOTH sides: the values the script
//! captured (proving the script could see them) and the engine's own transform
//! afterwards (proving the writes landed where a renderer would look). A
//! script-only check would pass if the setter wrote the global value straight
//! into the local slot, which is the bug the composition against the parent
//! exists to avoid.

use gdcore::math::Vector3;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::MainLoop;
use gdvariant::Variant;

/// A translated parent with a translated child, so a correct `global_position`
/// is the SUM and a naive local read is visibly wrong.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Rig" type="Node3D"]
position = Vector3(10, 20, 30)

[node name="Probe" type="Node3D" parent="."]
position = Vector3(1, 2, 3)
"#;

/// Reads `$Probe`'s global surface and exercises the methods.
///
/// Everything is captured into script vars so the test can assert on what the
/// SCRIPT saw, not only on engine state.
const RIG_GD: &str = "\
extends Node3D

var probe_global = Vector3(0, 0, 0)
var visible_before = false
var visible_after = true
var restored = Vector3(0, 0, 0)
var parent_name = \"\"

func _ready():
    probe_global = $Probe.global_position
    visible_before = $Probe.is_visible_in_tree()

    # A whole-transform round trip: read it, move the node elsewhere, put the
    # transform back, and confirm the position returns. No Transform3D literal
    # is needed because the value comes from the engine.
    var saved = $Probe.global_transform
    $Probe.global_position = Vector3(5, 5, 5)
    $Probe.global_transform = saved
    restored = $Probe.global_position

    $Probe.global_rotate(Vector3.UP, 1.5707963)
    parent_name = $Probe.get_parent_node_3d().name

    $Probe.hide()
    visible_after = $Probe.is_visible_in_tree()
";

fn script_vec3(tree: &SceneTree, node: NodeId, prop: &str) -> Vector3 {
    match tree
        .get_script(node)
        .expect("the rig has a script")
        .get_property(prop)
    {
        Some(Variant::Vector3(v)) => v,
        other => panic!("{prop} should be a Vector3, got {other:?}"),
    }
}

fn script_bool(tree: &SceneTree, node: NodeId, prop: &str) -> bool {
    match tree
        .get_script(node)
        .expect("the rig has a script")
        .get_property(prop)
    {
        Some(Variant::Bool(b)) => b,
        other => panic!("{prop} should be a Bool, got {other:?}"),
    }
}

fn close(a: Vector3, b: Vector3) -> bool {
    (a.x - b.x).abs() < 0.001 && (a.y - b.y).abs() < 0.001 && (a.z - b.z).abs() < 0.001
}

/// **The acceptance test.**
#[test]
fn node3d_script_global_surface() {
    let scene = PackedScene::from_tscn(SCENE).expect("the .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let rig = add_packed_scene_to_tree(&mut tree, root, &scene).expect("the scene instances");
    let probe = tree
        .get_node_by_path("/root/Rig/Probe")
        .expect("the child is in the tree");

    // Fixture check: the parent really is translated, so `global_position`
    // differing from `position` is meaningful rather than incidental.
    assert!(
        close(
            gdscene::node3d::get_position(&tree, probe),
            Vector3::new(1.0, 2.0, 3.0)
        ),
        "the probe's LOCAL position should be its .tscn value"
    );

    tree.attach_script(
        rig,
        Box::new(GDScriptNodeInstance::from_source(RIG_GD, rig).expect("the script parses")),
    );

    let mut ml = MainLoop::new(tree);
    ml.enter_tree_with_scripts(rig);
    ml.step(1.0 / 60.0);

    let tree = ml.tree();

    // -- the property read --------------------------------------------------
    assert!(
        close(
            script_vec3(tree, rig, "probe_global"),
            Vector3::new(11.0, 22.0, 33.0)
        ),
        "`$Probe.global_position` must be the parent offset PLUS the local \
         position, i.e. (11,22,33); got {:?}. Before this bead it read back Nil \
         because nothing computed it.",
        script_vec3(tree, rig, "probe_global")
    );

    // -- the whole-transform round trip -------------------------------------
    assert!(
        close(
            script_vec3(tree, rig, "restored"),
            Vector3::new(11.0, 22.0, 33.0)
        ),
        "assigning a previously-read `global_transform` back must restore the \
         node's global position; got {:?}",
        script_vec3(tree, rig, "restored")
    );
    // And the engine agrees — this is the half a script-only assertion would
    // miss. If the setter had written the global value into the LOCAL slot, the
    // node would sit at parent + target instead of at target.
    assert!(
        close(
            gdscene::node3d::get_global_transform(tree, probe).origin,
            Vector3::new(11.0, 22.0, 33.0)
        ),
        "the engine's own global transform must agree with the script; got {:?}",
        gdscene::node3d::get_global_transform(tree, probe).origin
    );

    // -- the method surface -------------------------------------------------
    assert!(
        script_bool(tree, rig, "visible_before"),
        "`is_visible_in_tree()` must be reachable and true to start — it had no \
         dispatch arm at all, so it answered Nil"
    );
    assert!(
        !script_bool(tree, rig, "visible_after"),
        "`hide()` must make `is_visible_in_tree()` false"
    );
    assert_eq!(
        tree.get_script(rig)
            .unwrap()
            .get_property("parent_name")
            .unwrap_or(Variant::Nil),
        Variant::String("Rig".to_string()),
        "`get_parent_node_3d()` must return the nearest Node3D ancestor"
    );

    // `global_rotate` turns about a global axis through the node's own origin,
    // so the basis must have changed while the position must NOT have.
    let basis = gdscene::node3d::get_global_transform(tree, probe).basis;
    assert!(
        (basis.x.x - 1.0).abs() > 0.001 || (basis.x.z).abs() > 0.001,
        "`global_rotate(Vector3.UP, PI/2)` must change the basis; it is still \
         {:?}",
        basis.x
    );
    assert!(
        close(
            gdscene::node3d::get_global_transform(tree, probe).origin,
            Vector3::new(11.0, 22.0, 33.0)
        ),
        "a rotation about the node's own origin must not move it"
    );
}

/// The 3D surface must decline a 2D node, or a `Sprite2D`'s `global_position`
/// read would come back as a `Vector3` where the script expects a `Vector2`.
///
/// `global_position` is a property of BOTH Node2D and Node3D, and the 2D side is
/// already served elsewhere — so the `is_class("Node3D")` gate is what keeps the
/// two from colliding.
#[test]
fn the_3d_surface_declines_a_2d_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let sprite = tree
        .add_child(root, gdscene::node::Node::new("Spr", "Sprite2D"))
        .unwrap();

    assert!(
        gdscene::node3d::get_derived_property(&tree, sprite, "global_position").is_none(),
        "the Node3D reader must decline a Sprite2D, or it would answer a \
         Vector3 for a 2D node's global_position"
    );
    assert!(
        gdscene::node3d::call_node3d_method(&mut tree, sprite, "is_visible_in_tree", &[]).is_none(),
        "the Node3D method table must decline a 2D node"
    );
}

/// A node that opted out of inheriting its parent's transform must report the
/// position it is actually drawn at, which is why the global reads go through
/// `get_global_transform_top_level` rather than the plain walk.
#[test]
fn a_top_level_node_ignores_its_parents_transform() {
    let scene = PackedScene::from_tscn(SCENE).expect("the .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("the scene instances");
    let probe = tree.get_node_by_path("/root/Rig/Probe").expect("child");

    gdscene::node3d::set_top_level(&mut tree, probe, true);
    match gdscene::node3d::get_derived_property(&tree, probe, "global_position") {
        Some(Variant::Vector3(v)) => assert!(
            close(v, Vector3::new(1.0, 2.0, 3.0)),
            "a top_level node's global position is its OWN local position, not \
             the parent sum; got {v:?}"
        ),
        other => panic!("global_position should be a Vector3, got {other:?}"),
    }
}
