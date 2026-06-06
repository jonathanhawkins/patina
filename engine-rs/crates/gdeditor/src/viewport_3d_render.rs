//! Building the 3D viewport's render list from the scene's Node3D content.
//!
//! [`Viewport3D::collect_scene_content`](crate::viewport_3d::Viewport3D::collect_scene_content)
//! gathers the spatial nodes; this module turns them into concrete draw items
//! the renderer can mount: a **mesh** for mesh-bearing nodes (`MeshInstance3D`),
//! or a **primitive marker** (camera frustum, light icon, or a generic box) for
//! spatial nodes with no mesh so cameras, lights, and empty `Node3D`s are still
//! visible and pickable in the editor — matching Godot's 3D viewport.
//!
//! It lives in its own module (over the public `viewport_3d` API) to compose
//! with the viewport without growing that already-large file.

use gdcore::math3d::Transform3D;
use gdscene::SceneTree;

use crate::viewport_3d::{Node3DRenderable, Viewport3D};

/// The kind of content drawn for a spatial node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Render3DKind {
    /// A mesh resource to render, identified by its resource path.
    Mesh { path: String },
    /// A built-in primitive marker drawn for a spatial node that has no mesh
    /// (cameras, lights, empty `Node3D`s, collision shapes) so it stays visible
    /// and pickable in the editor.
    Primitive { shape: PrimitiveShape },
}

/// The primitive marker drawn for a non-mesh spatial node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveShape {
    /// A camera frustum/icon (for `Camera3D`).
    Camera,
    /// A light icon (for the `Light3D` family).
    Light,
    /// A generic box marker for any other spatial node.
    Box,
}

/// One drawable item in the 3D viewport's render list.
#[derive(Debug, Clone, PartialEq)]
pub struct Render3DItem {
    /// The source node's raw id.
    pub node_id: u64,
    /// The node's global (world-space) transform.
    pub global_transform: Transform3D,
    /// What to draw for this node.
    pub kind: Render3DKind,
}

impl Viewport3D {
    /// Builds the 3D viewport's render list from `tree`: each **visible**
    /// `Node3D`-derived node becomes a [`Render3DItem`] — a mesh for mesh-bearing
    /// nodes, or a primitive marker otherwise. Hidden nodes are skipped (nothing
    /// to draw), and items appear in scene-tree order.
    pub fn build_render_list(&self, tree: &SceneTree) -> Vec<Render3DItem> {
        self.collect_scene_content(tree)
            .into_iter()
            .filter(|r| r.visible)
            .map(render_item_for)
            .collect()
    }
}

/// Maps a collected spatial node to its draw item.
fn render_item_for(r: Node3DRenderable) -> Render3DItem {
    let kind = match r.mesh_path {
        Some(path) => Render3DKind::Mesh { path },
        None => Render3DKind::Primitive {
            shape: primitive_for_class(&r.class_name),
        },
    };
    Render3DItem {
        node_id: r.node_id,
        global_transform: r.global_transform,
        kind,
    }
}

/// Chooses the primitive marker for a non-mesh spatial node from its class.
fn primitive_for_class(class: &str) -> PrimitiveShape {
    if class.contains("Camera") {
        PrimitiveShape::Camera
    } else if class.contains("Light") {
        PrimitiveShape::Light
    } else {
        PrimitiveShape::Box
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport_3d::Viewport3D;
    use gdscene::node::Node;
    use gdscene::node3d;

    fn find<'a>(items: &'a [Render3DItem], node_id: u64) -> Option<&'a Render3DItem> {
        items.iter().find(|i| i.node_id == node_id)
    }

    /// Acceptance (pat-kka71.2): the 3D viewport builds a render list from the
    /// scene's Node3D content — meshes for mesh-bearing nodes, primitive markers
    /// for cameras/lights/empties — skipping hidden and non-spatial nodes.
    #[test]
    fn editor3d_renders_node3d_content() {
        gdobject::class_db::register_3d_classes();
        gdobject::class_db::register_2d_classes();

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mesh_id = tree
            .add_child(root, Node::new("Crate", "MeshInstance3D"))
            .unwrap();
        let cam_id = tree.add_child(root, Node::new("Cam", "Camera3D")).unwrap();
        let light_id = tree
            .add_child(root, Node::new("Sun", "DirectionalLight3D"))
            .unwrap();
        let empty_id = tree.add_child(root, Node::new("Pivot", "Node3D")).unwrap();
        let hidden_id = tree
            .add_child(root, Node::new("HiddenMesh", "MeshInstance3D"))
            .unwrap();
        // A non-spatial node must never appear in the 3D render list.
        let sprite2d_id = tree
            .add_child(root, Node::new("Sprite", "Sprite2D"))
            .unwrap();

        node3d::set_position(&mut tree, mesh_id, gdcore::math::Vector3::new(1.0, 2.0, 3.0));
        node3d::set_mesh_path(&mut tree, mesh_id, "res://crate.obj");
        node3d::set_visible(&mut tree, hidden_id, false);

        let vp = Viewport3D::default();
        let items = vp.build_render_list(&tree);
        let ids: Vec<u64> = items.iter().map(|i| i.node_id).collect();

        // Hidden and non-spatial nodes are excluded.
        assert!(!ids.contains(&hidden_id.raw()), "hidden node is not drawn");
        assert!(!ids.contains(&sprite2d_id.raw()), "2D node is not drawn");

        // The mesh node renders its mesh, at its global transform.
        let mesh = find(&items, mesh_id.raw()).expect("mesh item present");
        assert_eq!(mesh.kind, Render3DKind::Mesh { path: "res://crate.obj".to_string() });
        assert_eq!(mesh.global_transform.origin, gdcore::math::Vector3::new(1.0, 2.0, 3.0));

        // Camera and light render primitive markers.
        assert_eq!(
            find(&items, cam_id.raw()).unwrap().kind,
            Render3DKind::Primitive { shape: PrimitiveShape::Camera }
        );
        assert_eq!(
            find(&items, light_id.raw()).unwrap().kind,
            Render3DKind::Primitive { shape: PrimitiveShape::Light }
        );

        // A plain, mesh-less Node3D renders a generic box marker.
        assert_eq!(
            find(&items, empty_id.raw()).unwrap().kind,
            Render3DKind::Primitive { shape: PrimitiveShape::Box }
        );
    }
}
