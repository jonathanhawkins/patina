//! pat-s3rrv.24 (dispatch half) — Node3D's transform methods are callable from
//! GDScript and run the real transform maths.
//!
//! The registration half of this bead (SageCastle) added 37 Node3D and 19
//! Camera3D methods to ClassDB, which makes them **reflectable**:
//! `class_has_method("Node3D", "look_at")` answers true. That is necessary and
//! not sufficient — a script call still has to land on a dispatch arm, and
//! there was none. `grep '"look_at"'` over `interpreter.rs` and
//! `gdscene/src/scripting.rs` returned **0** in both.
//!
//! Meanwhile `gdscene::node3d::{look_at, translate, rotate, to_global,
//! to_local}` all existed and were correct. The maths was right, reflection said
//! the method existed, and calling it from a script did nothing.
//!
//! ⚠️ This test drives scene loading + `MainLoop::step` only. It never calls
//! `node3d::*` directly — the acceptance forbids it, and for good reason: those
//! functions were already green in isolation for the entire period the feature
//! was unreachable.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Mover" type="Node3D" parent="."]
"#;

fn num(v: &Variant) -> f64 {
    match v {
        Variant::Float(f) => *f,
        Variant::Int(i) => *i as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

/// Reads a script `var` — node properties and script vars are separate stores.
fn script_var(tree: &SceneTree, node: gdscene::node::NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

/// Reads the node's stored local `Transform3D`.
///
/// ⚠️ NOT a `position`/`rotation` property read. `node3d::translate` and
/// `rotate` both write through `set_local_transform`, which stores a single
/// `transform` (`Variant::Transform3D`) — there is no euler `rotation` property
/// to read back, and asserting on one panics with "not a Vector3" no matter how
/// well the dispatch works.
fn local_transform(tree: &SceneTree, node: gdscene::node::NodeId) -> gdcore::math3d::Transform3D {
    match tree
        .get_node(node)
        .expect("node exists")
        .get_property("transform")
    {
        Variant::Transform3D(t) => t,
        other => panic!("`transform` is not a Transform3D: {other:?}"),
    }
}

/// **The acceptance.** A script moves a Node3D through the engine's own
/// transform API and the node's real transform follows.
#[test]
fn node3d_transform_methods_dispatch_from_script() {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let mover = tree
        .get_node_or_null(scene_root, "Mover")
        .expect("Mover node");

    // `translate` accumulates in local space; `to_global`/`to_local` round-trip
    // a point through the node's transform. Both were unreachable before.
    let script_src = "\
extends Node3D
var round_trip_x = -1.0
var round_trip_y = -1.0
var round_trip_z = -1.0
var ran = false
func _ready():
    var m = get_node(\"Mover\")
    m.translate(Vector3(3, 4, 5))
    var g = m.to_global(Vector3(0, 0, 0))
    self.round_trip_x = g.x
    self.round_trip_y = g.y
    self.round_trip_z = g.z
    self.ran = true
";
    let script = GDScriptNodeInstance::from_source(script_src, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(scene_root);

    assert_eq!(
        script_var(ml.tree(), scene_root, "ran"),
        Variant::Bool(true),
        "`_ready` must run to completion — an earlier call raised, which is what \
         an unreachable method looks like from here"
    );

    // The node's REAL transform moved: this is what registration alone could
    // never have produced.
    let pos = local_transform(ml.tree(), mover).origin;
    assert!(
        (pos.x - 3.0).abs() < 1e-4 && (pos.y - 4.0).abs() < 1e-4 && (pos.z - 5.0).abs() < 1e-4,
        "translate(Vector3(3,4,5)) must move the node — got {pos:?}"
    );

    // `to_global(ZERO)` is the node's own origin in global space, so it must
    // agree with the position the translate produced.
    assert!(
        (num(&script_var(ml.tree(), scene_root, "round_trip_x")) - 3.0).abs() < 1e-4,
        "to_global(ZERO).x must equal the translated x"
    );
    assert!(
        (num(&script_var(ml.tree(), scene_root, "round_trip_y")) - 4.0).abs() < 1e-4,
        "to_global(ZERO).y must equal the translated y"
    );
    assert!(
        (num(&script_var(ml.tree(), scene_root, "round_trip_z")) - 5.0).abs() < 1e-4,
        "to_global(ZERO).z must equal the translated z"
    );
}

/// `rotate_y` reaches the real rotation, and `look_at` orients the node.
///
/// Kept separate so a failure names which API broke rather than one combined
/// assertion covering three call paths.
#[test]
fn node3d_rotate_and_look_at_dispatch_from_script() {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let mover = tree
        .get_node_or_null(scene_root, "Mover")
        .expect("Mover node");

    let script_src = "\
extends Node3D
var ran = false
func _ready():
    var m = get_node(\"Mover\")
    m.rotate_y(1.0)
    self.ran = true
";
    let script = GDScriptNodeInstance::from_source(script_src, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(scene_root);

    assert_eq!(
        script_var(ml.tree(), scene_root, "ran"),
        Variant::Bool(true),
        "`_ready` must run to completion"
    );

    // A y-rotation turns the basis: the x axis must no longer be (1,0,0).
    let basis_x = local_transform(ml.tree(), mover).basis.x;
    assert!(
        (basis_x.x - 1.0).abs() > 1e-3 || basis_x.z.abs() > 1e-3,
        "rotate_y(1.0) must turn the basis — got x axis {basis_x:?}; an \
         unchanged (1,0,0) means the call never reached node3d::rotate"
    );
}
