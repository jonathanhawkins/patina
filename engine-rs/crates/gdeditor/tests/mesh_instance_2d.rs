//! pat-0q2kx.248 (partial) — `MeshInstance2D` draws instead of being invisible.
//!
//! Godot ground truth: `scene/2d/mesh_instance_2d.cpp` draws the mesh with its
//! texture.
//!
//! `render_kind_for` had arms only for Sprite2D / Polygon2D / Line2D /
//! ColorRect / Label, so MeshInstance2D fell through to `Marker` and never
//! rendered.
//!
//! ⚠️ SCOPE: PRIMITIVE meshes only. `packed_scene.rs` flattens
//! `mesh = SubResource(..)` into `__viewport_mesh__ = "primitive:<Type>"`, so a
//! QuadMesh's four corners are derivable with no `.tscn` parsing. Real
//! `ArrayMesh` geometry needs vertex/uv/index flattening that does not exist
//! (there is no `__mesh__` companion) — those nodes still fall through to
//! Marker rather than drawing something wrong. MultiMeshInstance2D, CanvasGroup
//! and BackBufferCopy are untouched.

use gdeditor::viewport_2d_render::{collect_canvas_items, Render2DKind};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// `collect_canvas_items` gates on `node.is_class("CanvasItem")`, which
/// consults the ClassDB — an unpopulated DB makes every node invisible to the
/// render list regardless of its class name.
fn tree_with_mesh(primitive: Option<&str>) -> SceneTree {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let m = tree
        .add_child(root, Node::new("Mesh", "MeshInstance2D"))
        .unwrap();
    if let Some(p) = primitive {
        tree.get_node_mut(m).unwrap().set_property(
            "__viewport_mesh__",
            Variant::String(format!("primitive:{p}")),
        );
    }
    tree
}

fn kinds(tree: &SceneTree) -> Vec<Render2DKind> {
    collect_canvas_items(tree)
        .into_iter()
        .map(|i| i.kind)
        .collect()
}

/// A QuadMesh MeshInstance2D produces Mesh geometry, not a Marker.
#[test]
fn a_quad_mesh_node_produces_mesh_geometry() {
    let tree = tree_with_mesh(Some("QuadMesh"));
    let mesh = kinds(&tree)
        .into_iter()
        .find_map(|k| match k {
            Render2DKind::Mesh { vertices, uvs, .. } => Some((vertices, uvs)),
            _ => None,
        })
        .expect("MeshInstance2D must emit Mesh geometry, not fall through to Marker");

    let (vertices, uvs) = mesh;
    assert_eq!(vertices.len(), 4, "a quad has four corners");
    assert_eq!(uvs.len(), vertices.len(), "one UV per vertex");

    // Godot's QuadMesh is 1x1 centred on the origin, so the corners straddle
    // zero. A quad anchored at the origin instead would render offset by half
    // its size — visible, but in the wrong place.
    assert!(
        vertices.iter().any(|v| v.x < 0.0) && vertices.iter().any(|v| v.x > 0.0),
        "the quad must be CENTRED, got {vertices:?}"
    );
    assert!(vertices.iter().any(|v| v.y < 0.0) && vertices.iter().any(|v| v.y > 0.0));

    // UVs span the full texture.
    assert!(uvs.iter().any(|u| u.x == 0.0) && uvs.iter().any(|u| u.x == 1.0));
    assert!(uvs.iter().any(|u| u.y == 0.0) && uvs.iter().any(|u| u.y == 1.0));
}

/// A MeshInstance2D with NO primitive marker still falls through rather than
/// emitting empty geometry — an ArrayMesh must not silently draw nothing that
/// looks like a successful draw.
#[test]
fn an_unsupported_mesh_still_falls_through() {
    let tree = tree_with_mesh(None);
    assert!(
        !kinds(&tree)
            .iter()
            .any(|k| matches!(k, Render2DKind::Mesh { .. })),
        "with no primitive marker there is no geometry to draw, so the node \
         must NOT claim to be a drawable mesh"
    );

    // An unrecognised primitive behaves the same way.
    let other = tree_with_mesh(Some("ArrayMesh"));
    assert!(
        !kinds(&other)
            .iter()
            .any(|k| matches!(k, Render2DKind::Mesh { .. })),
        "ArrayMesh geometry is not reachable yet; it must not fake a quad"
    );
}

/// The mesh arm must not have disturbed the classes that already rendered.
#[test]
fn existing_node_kinds_are_unaffected() {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cr = tree
        .add_child(root, Node::new("Rect", "ColorRect"))
        .unwrap();
    tree.get_node_mut(cr)
        .unwrap()
        .set_property("color", Variant::Color(gdcore::math::Color::WHITE));

    assert!(
        kinds(&tree)
            .iter()
            .any(|k| matches!(k, Render2DKind::ColorRect { .. })),
        "adding the Mesh arm must not have shadowed ColorRect"
    );
}
