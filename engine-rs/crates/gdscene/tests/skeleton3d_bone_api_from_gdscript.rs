//! pat-s3rrv.23 — `Skeleton3D`'s bone API is reachable from a script.
//!
//! `gdscene::skeleton3d` has carried 38 working, unit-tested functions all
//! along — `get_bone_count`, `find_bone`, the pose getters and setters,
//! `get_bone_global_pose` (which `gdserver3d::skin` already consumes) — and
//! **no script could call one of them**. ClassDB registered `Skeleton3D` with
//! zero methods, so `class_has_method` answered false and the dispatch gate
//! rejected every call before it arrived.
//!
//! The registration has since been filled in, which opened that gate onto
//! **nothing**: no dispatch was ever written behind it. Green Rust tests over
//! an API no game could reach.
//!
//! ⚠️ These drive real GDScript through the tree, not the free functions. A
//! test that calls `skeleton3d::get_bone_count(...)` directly — which is what
//! the existing `bone_constraints.rs` does — passes identically whether or not
//! a script can reach any of it, which is exactly how this stayed invisible.

use gdcore::math::Vector3;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{skeleton3d, GDScriptNodeInstance};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A skeleton with `Root` -> `Chest` -> `Head`, plus a driver script on a
/// SEPARATE node.
///
/// ⚠️ The driver must not live on the skeleton itself: `call_script_method`
/// takes the running script OUT of the arena for the duration of the call, so a
/// script reaching back into its own node finds it checked out.
fn skeleton_and_driver(src: &str) -> (SceneTree, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let skel = tree
        .add_child(root, Node::new("Skel", "Skeleton3D"))
        .expect("skeleton adds");
    for name in ["Root", "Chest", "Head"] {
        skeleton3d::add_bone(&mut tree, skel, name);
    }
    skeleton3d::set_bone_parent(&mut tree, skel, 1, 0);
    skeleton3d::set_bone_parent(&mut tree, skel, 2, 1);

    let driver = tree
        .add_child(root, Node::new("Driver", "Node"))
        .expect("driver adds");
    tree.attach_script(
        driver,
        Box::new(GDScriptNodeInstance::from_source(src, driver).expect("script parses")),
    );
    (tree, skel, driver)
}

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no var `{name}`"))
        .default_value
}

fn int_var(tree: &SceneTree, node: NodeId, name: &str) -> i64 {
    match script_var(tree, node, name) {
        Variant::Int(i) => i,
        other => panic!("`{name}` is not an Int: {other:?}"),
    }
}

/// **The acceptance test**: a script reads the bone hierarchy.
#[test]
fn skeleton3d_bone_api_from_gdscript() {
    const SRC: &str = "\
extends Node
var count = -1
var head = -99
var missing = -99
var parent_of_head = -99
var name_ok = false
func run():
    var s = get_node(\"../Skel\")
    self.count = s.get_bone_count()
    self.head = s.find_bone(\"Head\")
    self.missing = s.find_bone(\"NoSuchBone\")
    self.parent_of_head = s.get_bone_parent(2)
    self.name_ok = s.get_bone_name(1) == \"Chest\"
";
    let (mut tree, _skel, driver) = skeleton_and_driver(SRC);
    tree.call_script_method(driver, "run", &[]);

    assert_eq!(
        int_var(&tree, driver, "count"),
        3,
        "`get_bone_count()` must answer from the script. -1 means the call \
         never dispatched — which is the entire bug: ClassDB says the method \
         exists, so the gate opens, and nothing implements it",
    );
    assert_eq!(int_var(&tree, driver, "head"), 2, "find_bone(\"Head\")");
    // ⚠️ Godot returns -1, NOT null: real code reads `if s.find_bone(n) != -1`.
    assert_eq!(
        int_var(&tree, driver, "missing"),
        -1,
        "an unknown bone name must be -1, the value Godot documents and every \
         ported script compares against",
    );
    assert_eq!(
        int_var(&tree, driver, "parent_of_head"),
        1,
        "Head's parent is Chest",
    );
    assert_eq!(script_var(&tree, driver, "name_ok"), Variant::Bool(true));
}

/// Pose writes from a script reach the engine's own pose storage
/// (pat-s3rrv.23).
///
/// This is the half that matters for animation: `gdserver3d::skin` skins from
/// `get_bone_global_pose`, so a script-driven pose has to land in the same
/// store the renderer reads, not in some script-side shadow copy.
#[test]
fn a_script_pose_write_reaches_the_engine_store() {
    const SRC: &str = "\
extends Node
var read_back = Vector3(0, 0, 0)
func run():
    var s = get_node(\"../Skel\")
    s.set_bone_pose_position(1, Vector3(1, 2, 3))
    self.read_back = s.get_bone_pose_position(1)
";
    let (mut tree, skel, driver) = skeleton_and_driver(SRC);
    tree.call_script_method(driver, "run", &[]);

    // The script's own read-back agrees…
    assert_eq!(
        script_var(&tree, driver, "read_back"),
        Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)),
        "the script must read back what it just wrote",
    );
    // …and so does the engine store the renderer actually samples.
    let engine = skeleton3d::get_bone_pose_position(&tree, skel, 1);
    assert_eq!(
        engine,
        Vector3::new(1.0, 2.0, 3.0),
        "the write must land in the engine's pose store — `gdserver3d::skin` \
         skins from these, so a script-side shadow copy would animate nothing",
    );
}

/// An out-of-range bone index answers Godot's default instead of panicking
/// (pat-s3rrv.23).
///
/// The implementations take `usize` while Godot's indices are signed, so every
/// index argument is validated before it is narrowed. ⚠️ Scale's neutral value
/// is ONE — answering `Vector3::ZERO` there would read as "this bone is
/// collapsed" rather than "no such bone".
#[test]
fn an_out_of_range_bone_index_is_survivable() {
    const SRC: &str = "\
extends Node
var parent = -99
var name_len = -1
var scale_x = -1.0
var enabled = true
func run():
    var s = get_node(\"../Skel\")
    self.parent = s.get_bone_parent(99)
    self.name_len = s.get_bone_name(99).length()
    self.scale_x = s.get_bone_pose_scale(99).x
    self.enabled = s.is_bone_enabled(99)
";
    let (mut tree, _skel, driver) = skeleton_and_driver(SRC);
    tree.call_script_method(driver, "run", &[]);

    assert_eq!(int_var(&tree, driver, "parent"), -1, "no bone, no parent");
    assert_eq!(int_var(&tree, driver, "name_len"), 0, "empty name");
    assert_eq!(
        script_var(&tree, driver, "scale_x"),
        Variant::Float(1.0),
        "an out-of-range scale must be ONE, not zero — zero reads as a \
         collapsed bone rather than a missing one",
    );
    assert_eq!(script_var(&tree, driver, "enabled"), Variant::Bool(false));
}
