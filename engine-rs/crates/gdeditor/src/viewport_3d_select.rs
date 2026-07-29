//! Click-to-select a Node3D in the 3D viewport.
//!
//! Clicking in the 3D viewport should select the spatial node under the cursor,
//! mirroring Godot. This composes the viewport's existing pick machinery
//! ([`Viewport3D::pickable_nodes`](crate::viewport_3d::Viewport3D::pickable_nodes)
//! + [`pick_node`](crate::viewport_3d::Viewport3D::pick_node)) with its
//! [`Selection3D`](crate::viewport_3d) state: a click casts a ray through the
//! scene's visible spatial nodes, and the topmost hit becomes the selection.
//!
//! Kept in its own module (over the public `viewport_3d` API) to avoid growing
//! the already-large `viewport_3d.rs`.

use gdscene::SceneTree;

use crate::viewport_3d::Viewport3D;

impl Viewport3D {
    /// Selects the topmost `Node3D` under a viewport click at pixel
    /// (`px`, `py`).
    ///
    /// Casts a pick ray through the scene's visible spatial nodes (via
    /// `pickable_nodes` + `pick_node`, using `pick_radius` as the per-node hit
    /// radius). On a hit, that node becomes the sole selection and its raw id is
    /// returned; a click on empty space clears the selection and returns `None`.
    pub fn click_select(
        &mut self,
        tree: &SceneTree,
        px: f32,
        py: f32,
        pick_radius: f32,
    ) -> Option<u64> {
        let nodes = self.pickable_nodes(tree);
        let hit = self.pick_node(px, py, &nodes, pick_radius);
        match hit {
            Some(result) => {
                self.selection.select(result.node_id);
                Some(result.node_id)
            }
            None => {
                self.selection.clear();
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport_3d::Viewport3D;
    use gdcore::math::Vector3;
    use gdscene::node::Node;
    use gdscene::node3d;

    /// Acceptance (pat-kka71.4): clicking in the 3D viewport selects the Node3D
    /// under the cursor; clicking empty space clears the selection.
    #[test]
    fn editor3d_click_select_node3d() {
        gdobject::class_db::register_3d_classes();

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = tree
            .add_child(root, Node::new("A", "MeshInstance3D"))
            .unwrap();
        let b = tree
            .add_child(root, Node::new("B", "MeshInstance3D"))
            .unwrap();
        // A sits at the camera's focus (origin); B is far off to the side so a
        // click on A's screen position can't accidentally hit it.
        node3d::set_position(&mut tree, a, Vector3::new(0.0, 0.0, 0.0));
        node3d::set_position(&mut tree, b, Vector3::new(100.0, 0.0, 0.0));

        let mut vp = Viewport3D::new(800, 600);
        // The default camera orbits the origin, so A projects near the center.
        let (ax, ay) = vp.world_to_screen(Vector3::new(0.0, 0.0, 0.0));

        // Clicking on A's screen position selects A (and only A).
        let hit = vp.click_select(&tree, ax, ay, 1.0);
        assert_eq!(hit, Some(a.raw()), "clicking A's screen position selects A");
        assert!(vp.selection.is_selected(a.raw()));
        assert!(!vp.selection.is_selected(b.raw()));

        // Clicking empty space (a far corner) selects nothing and clears.
        let miss = vp.click_select(&tree, 1.0, 1.0, 1.0);
        assert_eq!(miss, None, "clicking empty space selects nothing");
        assert!(
            vp.selection.primary().is_none(),
            "selection cleared on a miss"
        );
        assert!(!vp.selection.is_selected(a.raw()));
    }

    /// Rubber-band box-select (Godot parity): a screen rectangle selects every
    /// Node3D whose origin projects inside it, an empty area selects nothing, and
    /// a full-viewport rectangle catches everything.
    #[test]
    fn editor3d_box_select_node3d() {
        gdobject::class_db::register_3d_classes();

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = tree
            .add_child(root, Node::new("A", "MeshInstance3D"))
            .unwrap();
        let b = tree
            .add_child(root, Node::new("B", "MeshInstance3D"))
            .unwrap();
        node3d::set_position(&mut tree, a, Vector3::new(0.0, 0.0, 0.0));
        node3d::set_position(&mut tree, b, Vector3::new(6.0, 0.0, 0.0));

        let mut vp = Viewport3D::new(800, 600);
        let (ax, ay) = vp.world_to_screen(Vector3::new(0.0, 0.0, 0.0));
        let (bx, by) = vp.world_to_screen(Vector3::new(6.0, 0.0, 0.0));

        // A tight rectangle around A's projection selects A only.
        let hits = vp.box_select_hits(&tree, ax - 12.0, ay - 12.0, ax + 12.0, ay + 12.0);
        assert!(hits.contains(&a.raw()), "box around A selects A");
        assert!(
            !hits.contains(&b.raw()),
            "box around A excludes the distant B"
        );

        // Corner order doesn't matter; a rect spanning both projections selects both.
        let (min_x, max_x) = (ax.min(bx) - 12.0, ax.max(bx) + 12.0);
        let (min_y, max_y) = (ay.min(by) - 12.0, ay.max(by) + 12.0);
        let both = vp.box_select_hits(&tree, max_x, max_y, min_x, min_y);
        assert!(
            both.contains(&a.raw()) && both.contains(&b.raw()),
            "wide box selects both"
        );

        // An empty corner of the screen selects nothing.
        let none = vp.box_select_hits(&tree, 0.0, 0.0, 4.0, 4.0);
        assert!(none.is_empty(), "an empty screen region selects nothing");
    }
}
