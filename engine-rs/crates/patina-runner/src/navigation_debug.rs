//! Navmesh geometry emitted under `--debug-navigation` (pat-s3rrv.82).
//!
//! Godot's `--debug-navigation` draws navigation meshes in the running game.
//! `patina-runner` is a **headless** scene stepper — it runs N frames and dumps
//! tree state as JSON — so "draw the navmesh" here means putting the geometry
//! where a harness can assert on it, not rasterizing pixels.
//!
//! ⚠️ Lives in the LIB rather than in `main.rs` for the same reason
//! [`crate::debug_flags`] does: a binary's private helpers cannot be tested.

use gdscene::scene_tree::SceneTree;
use serde_json::{json, Value};

/// Every baked `NavigationRegion2D` in `tree`, as JSON.
///
/// Shape, one entry per region that has baked geometry:
///
/// ```json
/// { "node": "Level/Nav", "position": [12.0, 8.0],
///   "polygons": [[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0]]] }
/// ```
///
/// ⚠️ **Polygons are NODE-LOCAL and the region's position is reported beside
/// them, deliberately un-composed.** The runtime navigation map places regions
/// by their LOCAL translation (`main_loop.rs`), which the port audit records as
/// a bug — a region under an offset parent lands in the wrong world position.
/// Composing here would bake that open question into the output; emitting both
/// halves lets a consumer choose, and lets the fix land without changing this
/// contract.
///
/// Regions with no baked navmesh are omitted entirely rather than emitted with
/// an empty polygon list: "not baked" and "baked to nothing" are different
/// states and an author needs to tell them apart.
pub fn navmesh_draw_items(tree: &SceneTree) -> Value {
    let mut regions = Vec::new();
    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if !node.class_name().starts_with("NavigationRegion") {
            continue;
        }
        // The canonical reader, shared with the editor overlay, the editor
        // viewport chrome and the performance monitors (pat-s3rrv.82). The
        // runner reaches it because it depends on `gdscene`, not `gdeditor`.
        let polygons = gdscene::navigation::navmesh_polygons_local(node);
        if polygons.is_empty() {
            continue;
        }
        let position = gdscene::node2d::get_position(tree, id);
        regions.push(json!({
            "node": node.name(),
            "position": [position.x, position.y],
            "polygons": polygons
                .iter()
                .map(|poly| poly.iter().map(|p| json!([p.x, p.y])).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
        }));
    }
    Value::Array(regions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector2;
    use gdscene::node::Node;
    use gdvariant::Variant;

    fn region(name: &str, at: Vector2, baked: bool) -> Node {
        let mut n = Node::new(name, "NavigationRegion2D");
        n.set_property("position", Variant::Vector2(at));
        if baked {
            n.set_property(
                "__navmesh_vertices__",
                Variant::PackedVector2Array(vec![
                    Vector2::new(0.0, 0.0),
                    Vector2::new(4.0, 0.0),
                    Vector2::new(4.0, 4.0),
                ]),
            );
            n.set_property(
                "__navmesh_polygons__",
                Variant::Array(vec![Variant::PackedInt32Array(vec![0, 1, 2])]),
            );
        }
        n
    }

    /// 🔑 A BAKED REGION IS EMITTED WITH ITS GEOMETRY; AN UNBAKED ONE IS NOT.
    ///
    /// ⚠️ Omitted entirely rather than emitted with an empty polygon list.
    /// "Not baked" and "baked to nothing" are different states, and a harness
    /// that saw an empty entry could not tell a missing bake from a bake that
    /// produced no faces.
    #[test]
    fn navmesh_draw_items_emits_only_baked_regions() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.add_child(root, region("Baked", Vector2::new(12.0, 8.0), true))
            .unwrap();
        tree.add_child(root, region("Unbaked", Vector2::ZERO, false))
            .unwrap();

        let items = navmesh_draw_items(&tree);
        let arr = items.as_array().expect("an array of regions");
        assert_eq!(arr.len(), 1, "only the baked region is emitted: {items}");
        assert_eq!(arr[0]["node"], "Baked");
        assert_eq!(arr[0]["position"], serde_json::json!([12.0, 8.0]));
        assert_eq!(
            arr[0]["polygons"],
            serde_json::json!([[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0]]]),
            "polygons are node-LOCAL; the position is reported beside them"
        );
    }

    /// 🔑 THE POLYGONS ARE NOT PRE-OFFSET BY THE REGION'S POSITION.
    ///
    /// ⚠️ The runtime navigation map places regions by their LOCAL translation,
    /// which the port audit records as a bug. Composing position into the
    /// polygons here would bake that open question into the runner's output
    /// contract; emitting both halves separately lets the fix land without
    /// changing it. A region at a non-zero position is the only way to see the
    /// difference, which is why this one sits at (12, 8).
    #[test]
    fn the_emitted_polygons_stay_in_node_local_space() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.add_child(root, region("Offset", Vector2::new(12.0, 8.0), true))
            .unwrap();

        let items = navmesh_draw_items(&tree);
        let first = &items[0]["polygons"][0][0];
        assert_eq!(
            first,
            &serde_json::json!([0.0, 0.0]),
            "the first vertex is local (0,0); if it were (12,8) the position \
             had been composed in"
        );
    }

    /// A scene with no navigation regions emits an empty array, not `null`.
    #[test]
    fn a_scene_without_regions_emits_an_empty_array() {
        let tree = SceneTree::new();
        assert_eq!(navmesh_draw_items(&tree), serde_json::json!([]));
    }
}
