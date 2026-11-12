//! pat-0q2kx.197 — `RemoteTransform3D` forwards its transform to `remote_path`.
//!
//! The per-channel math already existed and was careful:
//! `node3d::remote_target_transform` handles the awkward partial-flag cases
//! (rotation-only keeps the target's basis column *lengths* and takes our
//! *directions*; scale-only is the mirror). But **every one of its callers was
//! inside `#[cfg(test)] mod tests`** — `main_loop` had no RemoteTransform3D
//! reference at all, and the class was registered bare, with none of its five
//! properties. So in a real scene the node was completely inert: a `.tscn`
//! carrying `remote_path = NodePath("…")` had nowhere to store it, and nothing
//! would have read it anyway.
//!
//! Godot: `upstream/godot/scene/3d/remote_transform_3d.cpp` — pushes on
//! `NOTIFICATION_TRANSFORM_CHANGED`, filtered by the three update flags.

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const EPSILON: f32 = 1e-4;

fn approx_vec3(a: Vector3, b: Vector3) -> bool {
    (a.x - b.x).abs() < EPSILON && (a.y - b.y).abs() < EPSILON && (a.z - b.z).abs() < EPSILON
}

fn set_remote_path(tree: &mut SceneTree, node: gdscene::node::NodeId, path: &str) {
    tree.get_node_mut(node).unwrap().set_property(
        "remote_path",
        Variant::NodePath(gdcore::node_path::NodePath::new(path)),
    );
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** A RemoteTransform3D with `update_rotation = false`
/// drives its sibling's position every frame and leaves its rotation alone;
/// clearing `remote_path` stops the forwarding.
#[test]
fn remote_transform3d_drives_target() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let remote = tree
        .add_child(root, Node::new("Remote", "RemoteTransform3D"))
        .unwrap();
    let target = tree
        .add_child(root, Node::new("Mesh", "MeshInstance3D"))
        .unwrap();

    // The target carries a rotation of its own, which must survive untouched.
    let target_rotation = Vector3::new(0.0, 0.5, 0.0);
    node3d::set_rotation(&mut tree, target, target_rotation);

    set_remote_path(&mut tree, remote, "../Mesh");
    tree.get_node_mut(remote)
        .unwrap()
        .set_property("update_rotation", Variant::Bool(false));

    let mut ml = MainLoop::new(tree);

    // -- the sibling tracks the source's position, frame by frame ------------
    //
    // Moving the source between steps is what a script animating it would do;
    // driving it directly keeps this test on the wiring rather than on the
    // scripting bridge.
    for step in 1..=3 {
        let p = Vector3::new(step as f32 * 5.0, step as f32, -(step as f32));
        node3d::set_position(ml.tree_mut(), remote, p);
        ml.step(0.016);

        let got = node3d::get_global_transform(ml.tree(), target).origin;
        assert!(
            approx_vec3(got, p),
            "frame {step}: the target must track the source's global position, \
             wanted {p:?}, got {got:?}",
        );
    }

    // -- …and its rotation is never touched ----------------------------------
    let rot = node3d::get_rotation(ml.tree(), target);
    assert!(
        approx_vec3(rot, target_rotation),
        "update_rotation = false must leave the target's rotation alone, \
         wanted {target_rotation:?}, got {rot:?}",
    );

    // -- clearing remote_path stops the forwarding ---------------------------
    let parked = node3d::get_global_transform(ml.tree(), target).origin;
    set_remote_path(ml.tree_mut(), remote, "");
    node3d::set_position(ml.tree_mut(), remote, Vector3::new(999.0, 999.0, 999.0));
    ml.step(0.016);

    let after = node3d::get_global_transform(ml.tree(), target).origin;
    assert!(
        approx_vec3(after, parked),
        "an empty remote_path stops forwarding; the target must stay at \
         {parked:?}, got {after:?}",
    );
}

// ===========================================================================
// Flags and coordinate space
// ===========================================================================

/// With every flag at its default the whole transform is copied — Godot's fast
/// path, and the common case.
#[test]
fn all_flags_default_copies_the_whole_transform() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let remote = tree
        .add_child(root, Node::new("Remote", "RemoteTransform3D"))
        .unwrap();
    let target = tree.add_child(root, Node::new("T", "Node3D")).unwrap();
    set_remote_path(&mut tree, remote, "../T");

    node3d::set_position(&mut tree, remote, Vector3::new(1.0, 2.0, 3.0));
    node3d::set_rotation(&mut tree, remote, Vector3::new(0.0, 0.75, 0.0));
    node3d::set_scale(&mut tree, remote, Vector3::new(2.0, 2.0, 2.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);

    assert!(
        approx_vec3(
            node3d::get_position(ml.tree(), target),
            Vector3::new(1.0, 2.0, 3.0)
        ),
        "position copied",
    );
    assert!(
        approx_vec3(
            node3d::get_rotation(ml.tree(), target),
            Vector3::new(0.0, 0.75, 0.0)
        ),
        "rotation copied",
    );
    assert!(
        approx_vec3(
            node3d::get_scale(ml.tree(), target),
            Vector3::new(2.0, 2.0, 2.0)
        ),
        "scale copied",
    );
}

/// `update_position = false` pushes orientation without dragging the target
/// off its own position.
#[test]
fn update_position_false_leaves_the_target_where_it_is() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let remote = tree
        .add_child(root, Node::new("Remote", "RemoteTransform3D"))
        .unwrap();
    let target = tree.add_child(root, Node::new("T", "Node3D")).unwrap();
    set_remote_path(&mut tree, remote, "../T");
    tree.get_node_mut(remote)
        .unwrap()
        .set_property("update_position", Variant::Bool(false));

    node3d::set_position(&mut tree, target, Vector3::new(10.0, 0.0, 0.0));
    node3d::set_position(&mut tree, remote, Vector3::new(-4.0, -4.0, -4.0));
    node3d::set_rotation(&mut tree, remote, Vector3::new(0.0, 1.0, 0.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);

    assert!(
        approx_vec3(
            node3d::get_position(ml.tree(), target),
            Vector3::new(10.0, 0.0, 0.0)
        ),
        "the target keeps its own position",
    );
    assert!(
        approx_vec3(
            node3d::get_rotation(ml.tree(), target),
            Vector3::new(0.0, 1.0, 0.0)
        ),
        "…but still receives the rotation",
    );
}

/// With `use_global_coordinates = false` Godot copies the *local* transform, so
/// a source nested under a moved parent hands over its local offset rather than
/// where it ended up in the world.
#[test]
fn local_mode_copies_the_local_transform() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let holder = tree.add_child(root, Node::new("Holder", "Node3D")).unwrap();
    node3d::set_position(&mut tree, holder, Vector3::new(100.0, 0.0, 0.0));

    let remote = tree
        .add_child(holder, Node::new("Remote", "RemoteTransform3D"))
        .unwrap();
    let target = tree.add_child(root, Node::new("T", "Node3D")).unwrap();
    {
        let n = tree.get_node_mut(remote).unwrap();
        n.set_property("use_global_coordinates", Variant::Bool(false));
    }
    set_remote_path(&mut tree, remote, "/root/T");
    node3d::set_position(&mut tree, remote, Vector3::new(3.0, 0.0, 0.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);

    let got = node3d::get_global_transform(ml.tree(), target).origin;
    assert!(
        approx_vec3(got, Vector3::new(3.0, 0.0, 0.0)),
        "local mode copies the local offset (3,0,0), not the global (103,0,0); got {got:?}",
    );
}

/// A global push onto a *nested* target must subtract that target's parent
/// chain, or it overshoots by the parent's offset every single frame.
#[test]
fn global_push_accounts_for_the_target_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let remote = tree
        .add_child(root, Node::new("Remote", "RemoteTransform3D"))
        .unwrap();
    node3d::set_position(&mut tree, remote, Vector3::new(50.0, 0.0, 0.0));

    let holder = tree.add_child(root, Node::new("Holder", "Node3D")).unwrap();
    node3d::set_position(&mut tree, holder, Vector3::new(20.0, 0.0, 0.0));
    let target = tree.add_child(holder, Node::new("T", "Node3D")).unwrap();

    set_remote_path(&mut tree, remote, "/root/Holder/T");

    let mut ml = MainLoop::new(tree);
    // Two steps: an overshoot bug compounds, so a second frame catches a fix
    // that only happens to be right once.
    ml.step(0.016);
    ml.step(0.016);

    let global = node3d::get_global_transform(ml.tree(), target).origin;
    assert!(
        approx_vec3(global, Vector3::new(50.0, 0.0, 0.0)),
        "the nested target must land on the source's GLOBAL position; got {global:?}",
    );
    let local = node3d::get_position(ml.tree(), target);
    assert!(
        approx_vec3(local, Vector3::new(30.0, 0.0, 0.0)),
        "its local position compensates for the holder's offset; got {local:?}",
    );
}

/// A `remote_path` pointing at the node itself must be ignored, not looped.
#[test]
fn self_target_is_ignored() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let remote = tree
        .add_child(root, Node::new("Remote", "RemoteTransform3D"))
        .unwrap();
    set_remote_path(&mut tree, remote, ".");
    node3d::set_position(&mut tree, remote, Vector3::new(1.0, 2.0, 3.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);
    ml.step(0.016);

    assert!(
        approx_vec3(
            node3d::get_position(ml.tree(), remote),
            Vector3::new(1.0, 2.0, 3.0)
        ),
        "a self-referential remote_path leaves the node where it was",
    );
}

/// The canonical setter keeps the four transform views in sync — writing only
/// `transform` is how `position`/`rotation`/`scale` drifted apart before
/// (pat-0q2kx.29), and a renderer reading `position` would miss the update.
#[test]
fn forwarding_keeps_every_transform_view_in_sync() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let remote = tree
        .add_child(root, Node::new("Remote", "RemoteTransform3D"))
        .unwrap();
    let target = tree.add_child(root, Node::new("T", "Node3D")).unwrap();
    set_remote_path(&mut tree, remote, "../T");
    node3d::set_position(&mut tree, remote, Vector3::new(7.0, 8.0, 9.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);

    let node = ml.tree().get_node(target).unwrap();
    match node.get_property("position") {
        Variant::Vector3(v) => assert!(
            approx_vec3(v, Vector3::new(7.0, 8.0, 9.0)),
            "the `position` view must be updated, got {v:?}",
        ),
        other => panic!("expected a Vector3 position, got {other:?}"),
    }
    match node.get_property("transform") {
        Variant::Transform3D(t) => assert!(
            approx_vec3(t.origin, Vector3::new(7.0, 8.0, 9.0)),
            "the canonical `transform` must agree, got {:?}",
            t.origin,
        ),
        other => panic!("expected a Transform3D, got {other:?}"),
    }
}

/// The registration must actually carry the five properties, or a `.tscn`
/// setting `remote_path` has nowhere to put it. This is the half that was
/// missing: the class existed but was registered bare.
#[test]
fn remote_transform3d_registers_its_properties() {
    gdobject::class_db::register_2d_classes();

    let info = gdobject::class_db::get_class_info("RemoteTransform3D")
        .expect("RemoteTransform3D must be registered");
    let names: Vec<&str> = info.properties.iter().map(|p| p.name.as_str()).collect();

    for expected in [
        "remote_path",
        "update_position",
        "update_rotation",
        "update_scale",
        "use_global_coordinates",
    ] {
        assert!(
            names.contains(&expected),
            "RemoteTransform3D must register `{expected}`; has {names:?}",
        );
    }
    assert_eq!(info.parent_class, "Node3D", "parent is Node3D");
}

/// Sanity: an identity-transform source still writes, and a second frame does
/// not accumulate. Idempotence matters because this runs every frame forever.
#[test]
fn repeated_frames_do_not_accumulate() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let remote = tree
        .add_child(root, Node::new("Remote", "RemoteTransform3D"))
        .unwrap();
    let target = tree.add_child(root, Node::new("T", "Node3D")).unwrap();
    set_remote_path(&mut tree, remote, "../T");
    node3d::set_position(&mut tree, remote, Vector3::new(2.0, 0.0, 0.0));

    let mut ml = MainLoop::new(tree);
    for _ in 0..10 {
        ml.step(0.016);
    }

    let got = node3d::get_global_transform(ml.tree(), target).origin;
    assert!(
        approx_vec3(got, Vector3::new(2.0, 0.0, 0.0)),
        "ten frames must land on the same place as one, got {got:?}",
    );
    // Compared column-wise rather than with `assert_eq!` on the whole
    // Transform3D: the global push runs the parent's inverse through a matrix
    // multiply, so exact float equality would be testing the FPU, not the fix.
    let basis = node3d::get_global_transform(ml.tree(), target).basis;
    let identity = Transform3D::IDENTITY.basis;
    assert!(
        approx_vec3(basis.x, identity.x)
            && approx_vec3(basis.y, identity.y)
            && approx_vec3(basis.z, identity.z),
        "the basis stays identity rather than compounding, got {basis:?}",
    );
}
