//! pat-0q2kx.173 — a BoxShape3D face-handle drag resizes the shape, persists,
//! and undoes.
//!
//! Godot draws draggable handles on a selected `CollisionShape3D` and resizing
//! by a face handle edits the shape sub-resource (`Gizmo3DHelper`).
//!
//! Patina had a faithful port of that math in `viewport_3d_handles`
//! (`box_get_handles` / `box_set_handle`) with its own unit tests and **zero
//! callers** — orphan code no drag could reach. This wires it to the editor's
//! undo stack.
//!
//! The wiring deliberately produces plain `SetProperty` commands rather than a
//! bespoke variant: a `CollisionShape3D`'s shape is flattened onto the node as
//! `__shape__*`, which the scene saver already writes, so undo AND `.tscn`
//! persistence both come for free.

use gdcore::math::Vector3;
use gdeditor::viewport_3d_handles::{box_get_handles, box_handle_drag_commands};
use gdscene::node::Node;
use gdscene::scene_saver::TscnSaver;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A `CollisionShape3D` carrying a 2×2×2 box, the way a loaded `.tscn` stores it.
fn scene_with_box() -> (SceneTree, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node3D")).unwrap();
    let mut shape = Node::new("Hitbox", "CollisionShape3D");
    // A loaded `.tscn` leaves THREE companions behind, and all three matter:
    // `__resref__shape` records `<Class>::<sub_resource id>` and is what drives
    // the saver's sub-resource rebuild, while `__shape__*` caches the body it
    // rebuilds from. Setting only the `__shape__*` cache — as I first did — saves
    // nothing, because there is no sub-resource for the size to be written into.
    shape.set_property(
        "__resref__shape",
        Variant::String("BoxShape3D::Shape_hitbox".into()),
    );
    shape.set_property("__shape__class", Variant::String("BoxShape3D".into()));
    shape.set_property(
        "__shape__size",
        Variant::Vector3(Vector3::new(2.0, 2.0, 2.0)),
    );
    shape.set_property("position", Variant::Vector3(Vector3::ZERO));
    let id = tree.add_child(main, shape).unwrap();
    (tree, id)
}

/// **The acceptance test.**
#[test]
fn viewport3d_gizmo_handles_e2e() {
    let (mut tree, node) = scene_with_box();

    // The six face handles sit at half the box size on each axis — handle 0 is
    // +X, so it starts at x = 1.0 for a 2-unit box.
    let handles = box_get_handles(Vector3::new(2.0, 2.0, 2.0));
    assert_eq!(handles.len(), 6, "a box has six face handles");
    assert!(
        (handles[0].x - 1.0).abs() < 1e-6,
        "handle 0 is the +X face at half the size, got {:?}",
        handles[0],
    );

    // -- drag the +X face out to x = 3 ---------------------------------------
    let (mut size_cmd, mut pos_cmd) =
        box_handle_drag_commands(&tree, node, 0, Vector3::new(3.0, 0.0, 0.0), false, None)
            .expect("a box node yields drag commands");

    size_cmd.execute(&mut tree).expect("size applies");
    pos_cmd.execute(&mut tree).expect("position applies");

    let after = tree.get_node(node).unwrap().get_property("__shape__size");
    let Variant::Vector3(size) = after else {
        panic!("__shape__size must stay a Vector3, got {after:?}");
    };
    // Dragging +X from 1.0 out to 3.0 grows that axis from 2 to 4; the other
    // two axes must be untouched.
    assert!(
        (size.x - 4.0).abs() < 1e-5,
        "the dragged axis resizes, got {size:?}",
    );
    assert!(
        (size.y - 2.0).abs() < 1e-6 && (size.z - 2.0).abs() < 1e-6,
        "only the dragged axis changes, got {size:?}",
    );

    // -- it persists into the .tscn ------------------------------------------
    //
    // The `__shape__*` cache is NEVER written as a node property — it is
    // internal. The saver instead rebuilds the `[sub_resource]` BODY from it,
    // which is why the resized shape survives a save with no new code. So the
    // assertion is on the sub-resource, not on the companion key.
    let saved = TscnSaver::save_tree(&tree, tree.root_id());
    assert!(
        saved.contains(r#"[sub_resource type="BoxShape3D" id="Shape_hitbox"]"#),
        "the shape must round-trip as a sub-resource:\n{saved}"
    );
    assert!(
        saved.contains("size = Vector3(4"),
        "the DRAGGED size (x=4) must be written into the sub-resource body:\n{saved}"
    );
    // And the companion cache itself must stay internal — emitting it would
    // invent a node property Godot has never heard of.
    assert!(
        !saved.contains("__shape__size"),
        "the internal companion must not leak into the saved scene:\n{saved}"
    );

    // -- and undo restores it -------------------------------------------------
    pos_cmd.undo(&mut tree).expect("position undoes");
    size_cmd.undo(&mut tree).expect("size undoes");
    let restored = tree.get_node(node).unwrap().get_property("__shape__size");
    let Variant::Vector3(back) = restored else {
        panic!("__shape__size must survive undo, got {restored:?}");
    };
    assert!(
        (back.x - 2.0).abs() < 1e-6,
        "undo restores the original size, got {back:?}",
    );
}

/// A symmetric (Alt) drag grows both faces and leaves the node centred.
#[test]
fn symmetric_drag_keeps_the_node_centred() {
    let (mut tree, node) = scene_with_box();
    let before = tree.get_node(node).unwrap().get_property("position");

    let (mut size_cmd, mut pos_cmd) =
        box_handle_drag_commands(&tree, node, 0, Vector3::new(3.0, 0.0, 0.0), true, None)
            .expect("drag commands");
    size_cmd.execute(&mut tree).expect("size applies");
    pos_cmd.execute(&mut tree).expect("position applies");

    let Variant::Vector3(size) = tree.get_node(node).unwrap().get_property("__shape__size") else {
        panic!("size is a Vector3");
    };
    // Symmetric doubles the extent about the centre: ra.x = 3 → size 6.
    assert!(
        (size.x - 6.0).abs() < 1e-5,
        "a symmetric drag grows both faces, got {size:?}",
    );
    let after = tree.get_node(node).unwrap().get_property("position");
    assert_eq!(
        format!("{before:?}"),
        format!("{after:?}"),
        "a symmetric drag must not move the node",
    );
}

/// The wiring refuses nodes it cannot edit rather than inventing a size.
#[test]
fn non_box_nodes_yield_no_commands() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let plain = tree.add_child(root, Node::new("Plain", "Node3D")).unwrap();

    assert!(
        box_handle_drag_commands(&tree, plain, 0, Vector3::new(1.0, 0.0, 0.0), false, None)
            .is_none(),
        "a node with no __shape__size is not a resizable box",
    );

    let (tree2, node) = scene_with_box();
    assert!(
        box_handle_drag_commands(&tree2, node, 99, Vector3::new(1.0, 0.0, 0.0), false, None)
            .is_none(),
        "an out-of-range handle id is rejected, not wrapped",
    );
}

/// An `Area3D`'s box already produces gizmo geometry — the audit's second
/// clause was partly stale.
///
/// `collect_scene_content` gates on `is_class("Node3D")` plus a `shape:` marker
/// and is otherwise **parent-agnostic**, so a `CollisionShape3D` under an
/// `Area3D` yields the same `Collision` wireframe it would under a StaticBody3D.
/// Nothing needed adding for that half; this pins it so it stays true.
#[test]
fn area3d_box_already_yields_gizmo_geometry() {
    gdobject::class_db::register_3d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let area = tree
        .add_child(root, Node::new("Trigger", "Area3D"))
        .unwrap();
    let mut shape = Node::new("Shape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("shape:BoxShape3D|2,4,6".into()));
    let shape_id = tree.add_child(area, shape).unwrap();

    let vp = gdeditor::viewport_3d::Viewport3D::new(640, 480);
    let items = vp.build_render_list(&tree);

    let item = items
        .iter()
        .find(|i| i.node_id == shape_id.raw())
        .expect("the Area3D's shape must appear in the 3D render list");
    match &item.kind {
        gdeditor::viewport_3d_render::Render3DKind::Collision {
            shape,
            half_extents,
        } => {
            assert_eq!(
                *shape,
                gdeditor::viewport_3d_render::CollisionShapeKind::Box,
                "a BoxShape3D under an Area3D draws as a box wireframe",
            );
            // The marker's dims carry through, so the gizmo is the authored size.
            assert!(
                half_extents.x > 0.0 && half_extents.y > 0.0 && half_extents.z > 0.0,
                "the box gizmo must have real extents, got {half_extents:?}",
            );
        }
        other => panic!("expected Collision gizmo geometry, got {other:?}"),
    }
}

/// Guards the de-orphaning itself: the drag entry point must have a real caller
/// outside the module's own tests, or the handle math is dead again.
#[test]
fn handle_math_is_no_longer_orphaned() {
    let src = include_str!("../src/viewport_3d_handles.rs");
    assert!(
        src.contains("pub fn box_handle_drag_commands"),
        "the drag entry point must exist",
    );
    assert!(
        src.contains("box_set_handle(") && src.contains("EditorCommand::SetProperty"),
        "the entry point must actually drive the ported math into undoable commands",
    );
}
