//! pat-s3rrv.89 — CSG boolean trees reach the editor/Play render list.
//!
//! ⚠️ **The evaluator was never the gap.** `gdserver3d/src/csg.rs` is ~1150 lines
//! and `gdscene::render_server_3d::evaluate_csg_tree` already folds a whole CSG
//! subtree with union/intersection/subtraction. It was *private*, with its only
//! caller inside `RenderServer3DAdapter` — which is the RUNNER's 3D path, not the
//! editor's. So `grep -i csg` over the editor's viewport files found nothing and
//! every CSG node fell through `primitive_for_class` to a 0.4 grey marker cube:
//! a carefully authored boolean tree drew as a pile of identical boxes.
//!
//! ⚠️ Two rules had to travel with the evaluation, not just the mesh:
//! **only roots evaluate**, and **descendants draw nothing**. Getting the second
//! wrong is the subtle one — a subtracted sphere would still be drawn as its own
//! marker, sitting inside the hole it had just cut.

use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{editor_3d_render_inputs, Render3DKind};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

/// A box with a sphere carved out of it. `operation = 2` is Godot's Subtraction.
fn scene(operation: i32) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Solid" type="CSGBox3D" parent="."]
size = Vector3(2, 2, 2)

[node name="Hole" type="CSGSphere3D" parent="Solid"]
radius = 1.2
operation = {operation}
"#
    )
}

fn load(src: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

/// **The acceptance test for the CSG third of this bead.**
#[test]
fn csg_boolean_tree_renders_as_geometry() {
    gdobject::class_db::register_3d_classes();
    let tree = load(&scene(2));
    let viewport = Viewport3D::default();

    let items = editor_3d_render_inputs(&viewport, &tree).items;

    let solid = tree
        .get_node_by_path("/root/Main/Solid")
        .expect("the CSG root is in the scene");
    let hole = tree
        .get_node_by_path("/root/Main/Solid/Hole")
        .expect("the carved child is in the scene");

    let root_item = items
        .iter()
        .find(|i| i.node_id == solid.raw())
        .expect("the CSG root must produce a render item");
    match &root_item.kind {
        Render3DKind::Geometry { mesh } => {
            assert!(
                !mesh.vertices.is_empty(),
                "the folded CSG mesh must carry geometry",
            );
        }
        other => panic!(
            "the CSG root must render as generated Geometry, not {other:?} — a \
             Primitive here is the old behaviour, where every CSG node drew the \
             same 0.4 marker cube",
        ),
    }

    // 🔑 The descendant contributes to its root's mesh and draws NOTHING itself.
    assert!(
        !items.iter().any(|i| i.node_id == hole.raw()),
        "a CSG descendant must not produce its own item — otherwise the \
         subtracted sphere is drawn as a marker sitting inside the hole it cut",
    );
}

/// 🔑 The `operation` property actually changes the geometry.
///
/// ⚠️ Without this, every assertion above is satisfied by an implementation that
/// ignores `operation` entirely and always unions: the root would still emit a
/// non-empty `Geometry` mesh and the child would still be skipped. Comparing the
/// two meshes is what makes the boolean op load-bearing.
#[test]
fn the_csg_operation_changes_the_folded_mesh() {
    gdobject::class_db::register_3d_classes();
    let viewport = Viewport3D::default();

    let mesh_for = |op: i32| {
        let tree = load(&scene(op));
        let solid = tree.get_node_by_path("/root/Main/Solid").expect("root");
        let items = editor_3d_render_inputs(&viewport, &tree).items;
        let item = items
            .into_iter()
            .find(|i| i.node_id == solid.raw())
            .expect("the CSG root must produce a render item");
        match item.kind {
            Render3DKind::Geometry { mesh } => mesh,
            other => panic!("expected generated geometry, got {other:?}"),
        }
    };

    let union = mesh_for(0);
    let subtraction = mesh_for(2);

    assert!(
        !union.vertices.is_empty() && !subtraction.vertices.is_empty(),
        "both operations must produce geometry, or the comparison below is \
         between two empty meshes",
    );
    assert_ne!(
        union, subtraction,
        "subtracting a sphere must not produce the same mesh as unioning it — \
         identical meshes mean the `operation` property is being dropped",
    );
}
