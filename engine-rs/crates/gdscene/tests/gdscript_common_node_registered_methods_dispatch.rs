//! pat-mgixk.94 — registered-in-ClassDB node methods that returned Nil now
//! dispatch and change state the frame loop reads.
//!
//! # The failure mode this closes
//!
//! Each method below was registered in the ClassDB — `is_current` at
//! `class_db.rs:1669`/`:13960`, `add_resource` at `:8891`, `reset_bone_poses` at
//! `:3829` — with **no dispatch arm**. That pairing is worse than an outright
//! missing method: `has_method()` answers **true**, so a script guarding with it
//! proceeds, and the call then returns `Nil` silently. Nothing errors, nothing
//! happens.
//!
//! # Why each assertion reads engine state rather than a return value
//!
//! 🔑 A dispatch arm that accepted its arguments and stored them somewhere no
//! reader consults would satisfy any assertion on its own return value. So every
//! clause here is checked through a **different** reader than the one that wrote
//! it:
//!
//! * the listener flag is read back through the same tree-order scan
//!   `AudioServerBridge::resolve_listener_3d` uses to pick the ear for the next
//!   mix (`audio_server_bridge.rs:664-671`);
//! * the preloader mutators are read back through `has_resource` /
//!   `get_resource`, which were already dispatched and read the node's
//!   `resources` Dictionary — so a mutator writing anywhere else fails here;
//! * the bone poses are read back through `get_bone_pose`, the accessor the
//!   skinning pass uses.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Builds `World` carrying `script`, plus the nodes the clauses need, and runs
/// one frame so `_ready` has executed through the real lifecycle.
fn run(script: &str, extra: impl FnOnce(&mut SceneTree, NodeId)) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = tree.add_child(root, Node::new("World", "Node")).unwrap();
    extra(&mut tree, world);
    tree.attach_script(
        world,
        Box::new(GDScriptNodeInstance::from_source(script, world).expect("script parses")),
    );
    let mut ml = MainLoop::new(tree);
    ml.enter_tree_with_scripts(world);
    ml.step(1.0 / 60.0);
    (ml, world)
}

fn script_var(ml: &MainLoop, node: NodeId, prop: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("node has a script")
        // `get_property`, never `list_properties().default_value` — the latter
        // is the declared initialiser and never changes.
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

// ---------------------------------------------------------------------------

/// **The acceptance test.** Three registered-but-dead method families, each
/// driven from GDScript and observed through an independent reader.
#[test]
fn gdscript_common_node_registered_methods_dispatch() {
    const SCRIPT: &str = "\
extends Node
var l1_before = true
var l2_before = true
var l1_after = true
var l2_after = true
var l2_cleared = true
var pre_added = false
var pre_value = \"<unset>\"
var pre_removed = true
var pre_renamed = false
var pre_old_gone = true
var pose_moved = Vector3(0, 0, 0)
var pose_reset = Vector3(0, 0, 0)
func _ready():
\t# -- AudioListener3D: make_current must displace the other listener ----
\tself.l1_before = $L1.is_current()
\tself.l2_before = $L2.is_current()
\t$L2.make_current()
\tself.l1_after = $L1.is_current()
\tself.l2_after = $L2.is_current()
\t$L2.clear_current()
\tself.l2_cleared = $L2.is_current()
\t# -- ResourcePreloader: mutators must reach the readers' Dictionary ----
\t$Pre.add_resource(\"a\", \"res://tex.png\")
\tself.pre_added = $Pre.has_resource(\"a\")
\tself.pre_value = $Pre.get_resource(\"a\")
\t$Pre.rename_resource(\"a\", \"b\")
\tself.pre_renamed = $Pre.has_resource(\"b\")
\tself.pre_old_gone = $Pre.has_resource(\"a\")
\t$Pre.remove_resource(\"b\")
\tself.pre_removed = $Pre.has_resource(\"b\")
\t# -- Skeleton3D: reset_bone_poses must restore the rest ----------------
\t$Skel.set_bone_pose_position(0, Vector3(9, 0, 0))
\tself.pose_moved = $Skel.get_bone_pose_position(0)
\t$Skel.reset_bone_poses()
\tself.pose_reset = $Skel.get_bone_pose_position(0)
";

    let (ml, world) = run(SCRIPT, |tree, world| {
        // Two listeners, neither flagged: `is_current` must start false for both,
        // which is what makes the displacement below observable.
        tree.add_child(world, Node::new("L1", "AudioListener3D"))
            .unwrap();
        tree.add_child(world, Node::new("L2", "AudioListener3D"))
            .unwrap();
        tree.add_child(world, Node::new("Pre", "ResourcePreloader"))
            .unwrap();
        let skel = tree
            .add_child(world, Node::new("Skel", "Skeleton3D"))
            .unwrap();
        // One bone with a NON-identity rest, so "the pose returned to the rest"
        // is distinguishable from "the pose was zeroed". A zeroing bug would
        // land on 0.0; the rest is 3.0.
        gdscene::skeleton3d::add_bone(tree, skel, "root");
        let mut rest = gdcore::math3d::Transform3D::IDENTITY;
        rest.origin = gdcore::math::Vector3::new(3.0, 0.0, 0.0);
        gdscene::skeleton3d::set_bone_rest(tree, skel, 0, rest);
    });

    // -- (a) the listener flag, read through the bridge's own rule ------------
    assert_eq!(
        (
            script_var(&ml, world, "l1_before"),
            script_var(&ml, world, "l2_before")
        ),
        (Variant::Bool(false), Variant::Bool(false)),
        "precondition: with neither listener flagged, `is_current` must be false \
         for both. A `true` here means the arm reports something other than the \
         current-listener scan and the displacement below proves nothing"
    );
    assert_eq!(
        script_var(&ml, world, "l2_after"),
        Variant::Bool(true),
        "`make_current()` must make L2 the listener the next mix uses; false \
         means the call returned Nil as it did before this bead"
    );
    assert_eq!(
        script_var(&ml, world, "l1_after"),
        Variant::Bool(false),
        "…and L1 must STOP being current. Godot keeps one active listener per \
         viewport (`audio_listener_3d.cpp:116-124`); if both read true, two \
         nodes claim an ear the bridge can only grant to the earlier one"
    );
    assert_eq!(
        script_var(&ml, world, "l2_cleared"),
        Variant::Bool(false),
        "`clear_current()` must give the ear up again"
    );

    // -- (b) the preloader mutators, read through the existing readers --------
    assert_eq!(
        script_var(&ml, world, "pre_added"),
        Variant::Bool(true),
        "`add_resource(\"a\", ..)` must be visible to `has_resource`, which \
         reads the node's `resources` Dictionary. False means the mutator wrote \
         somewhere no reader consults — the exact trap of backing it with a \
         `ResourcePreloader` struct that nothing instantiates per node"
    );
    assert_eq!(
        script_var(&ml, world, "pre_value"),
        Variant::String("res://tex.png".to_string()),
        "…and `get_resource` must hand back the value as passed, not a \
         placeholder"
    );
    assert_eq!(
        script_var(&ml, world, "pre_renamed"),
        Variant::Bool(true),
        "`rename_resource(\"a\", \"b\")` must make \"b\" present"
    );
    assert_eq!(
        script_var(&ml, world, "pre_old_gone"),
        Variant::Bool(false),
        "…and must REMOVE the old key. A rename that only inserts would leave \
         both, which reads as a copy"
    );
    assert_eq!(
        script_var(&ml, world, "pre_removed"),
        Variant::Bool(false),
        "`remove_resource(\"b\")` must drop it again"
    );

    // -- (c) the bone pose, read through the skinning accessor ---------------
    // Read back through `get_bone_pose_position`, which returns a `Vector3`.
    // Deliberately NOT `get_bone_pose(0).origin.x`: the interpreter has no
    // member access for `Transform3D`, so that spelling would fail at runtime
    // regardless of whether the reset worked.
    let pose_x = |prop: &str| -> f32 {
        match script_var(&ml, world, prop) {
            Variant::Vector3(v) => v.x,
            other => panic!("`{prop}` should be a Vector3, got {other:?}"),
        }
    };
    let moved = pose_x("pose_moved");
    assert!(
        (moved - 9.0).abs() < 1e-4,
        "precondition: the script moved bone 0's pose to x=9, so the accessor \
         must read 9 before the reset; got {moved}. Without this, a reset that \
         did nothing would be indistinguishable from one that worked"
    );
    let reset = pose_x("pose_reset");
    assert!(
        (reset - 3.0).abs() < 1e-4,
        "`reset_bone_poses()` must restore bone 0 to its REST (x=3), the value \
         Godot writes from `bones[i].rest` (`skeleton_3d.cpp:908-914`); got \
         {reset}. 9 means the call returned Nil and the pose never moved back; \
         0 means the pose was zeroed rather than reset to the rest"
    );
}
