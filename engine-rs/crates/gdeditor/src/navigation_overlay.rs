//! Navigation debug overlay for the 2D viewport (pat-s3rrv.82).
//!
//! Godot's editor and its `--debug-navigation` runtime draw a navigation
//! region as **filled faces plus outlined edges**, in the colours declared at
//! `servers/navigation_2d/navigation_server_2d.cpp:231-256`.
//!
//! ⚠️ A separate module rather than more of `viewport_2d_render.rs` (3.4k lines,
//! and under active edit by other lanes). Everything here reads only the scene
//! tree and returns plain geometry, so it folds into whichever renderer consumes
//! it without carrying any of that file's state.

use gdcore::math::{Color, Vector2};
use gdscene::node::NodeId;
use gdscene::SceneTree;
use gdvariant::Variant;

use crate::viewport_2d::{CanvasOverlay, OverlaySettings};

/// `debug/shapes/navigation/2d/geometry_face_color` — translucent cyan.
pub const FACE_COLOR: Color = Color {
    r: 0.5,
    g: 1.0,
    b: 1.0,
    a: 0.4,
};
/// `debug/shapes/navigation/2d/geometry_edge_color` — opaque cyan.
pub const EDGE_COLOR: Color = Color {
    r: 0.5,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};
/// `debug/shapes/navigation/2d/geometry_face_disabled_color` — translucent grey.
pub const FACE_DISABLED_COLOR: Color = Color {
    r: 0.5,
    g: 0.5,
    b: 0.5,
    a: 0.4,
};
/// `debug/shapes/navigation/2d/geometry_edge_disabled_color` — opaque grey.
pub const EDGE_DISABLED_COLOR: Color = Color {
    r: 0.5,
    g: 0.5,
    b: 0.5,
    a: 1.0,
};

/// One filled navmesh face, in world space.
#[derive(Debug, Clone, PartialEq)]
pub struct NavFace {
    /// The polygon's points, in order.
    pub points: Vec<Vector2>,
    /// Fill colour — the disabled variant when the region is disabled.
    pub color: Color,
    /// The region node this face came from.
    pub region: NodeId,
}

/// One navmesh boundary edge segment, in world space.
#[derive(Debug, Clone, PartialEq)]
pub struct NavEdge {
    /// Segment start.
    pub from: Vector2,
    /// Segment end.
    pub to: Vector2,
    /// Edge colour — the disabled variant when the region is disabled.
    pub color: Color,
    /// The region node this edge came from.
    pub region: NodeId,
}

/// The navigation overlay's draw items for one frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NavigationOverlay {
    /// Filled faces, drawn under the edges.
    pub faces: Vec<NavFace>,
    /// Outlined edges, drawn over the faces.
    pub edges: Vec<NavEdge>,
}

impl NavigationOverlay {
    /// Whether the overlay would draw nothing.
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty() && self.edges.is_empty()
    }
}

/// Godot's `NavigationRegion2D.enabled`; absent means enabled.
fn region_enabled(node: &gdscene::node::Node) -> bool {
    match node.get_property("enabled") {
        Variant::Bool(b) => b,
        _ => true,
    }
}

/// A region's baked navmesh polygons in **node-local** space.
///
/// The single place `__navmesh_vertices__` and `__navmesh_polygons__` are turned
/// into usable geometry. [`navigation_overlay_2d`] adds the region's position on
/// top for the canvas item pipeline; the editor viewport's immediate-mode
/// renderer wants them local, because it has already mapped the node's GLOBAL
/// position into screen space and adds them relative to that.
///
/// ⚠️ Local on purpose. Handing out pre-offset points was how the double-offset
/// bug nearly shipped: a caller that already knows where the node is would apply
/// the position twice.
///
/// Polygons of fewer than three points are dropped, so every returned polygon is
/// drawable.
pub fn navigation_polygons_local(node: &gdscene::node::Node) -> Vec<Vec<Vector2>> {
    gdscene::navigation::navmesh_polygons_local(node)
}

/// The `navigation/polygons` and `navigation/edges` performance monitors:
/// how much baked navmesh geometry the scene actually holds.
///
/// ⚠️ Shares [`navigation_polygons_local`] with [`navigation_overlay_2d`] rather than
/// re-reading `__navmesh_polygons__` here. That parser carries two behaviours a
/// second reader would drift from: a region with vertices but no `polygons`
/// entry is ONE face over all of them, and out-of-range indices are dropped
/// rather than panicking. A monitor that disagreed with the overlay about how
/// many polygons a region has would be worse than no monitor.
///
/// ⚠️ Counted the same way the overlay draws: polygons of fewer than three
/// points are skipped entirely, and each surviving polygon is a CLOSED loop
/// contributing N edges, not N - 1.
///
/// ⚠️ Unlike the overlay this ignores the View-menu toggle and the region's
/// `enabled` flag — it reports what the scene contains, not what is being
/// drawn. Gating it on the toggle would make the number drop to zero when an
/// author merely hid the overlay.
///
/// `NavigationRegion3D` contributes nothing yet: the bake route writes
/// `PackedVector2Array` vertices, so no 3D region has baked geometry to count.
pub fn navigation_geometry_counts(tree: &SceneTree) -> (u64, u64) {
    let mut polygons = 0u64;
    let mut edges = 0u64;
    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if !node.class_name().starts_with("NavigationRegion") {
            continue;
        }
        for points in navigation_polygons_local(node) {
            polygons += 1;
            // A closed polygon of N points contributes N edges, not N - 1.
            edges += points.len() as u64;
        }
    }
    (polygons, edges)
}

/// Builds the navigation overlay for every `NavigationRegion2D` in the tree.
///
/// Returns an empty overlay when the toggle is off — the View menu's
/// `CanvasOverlay::NavigationRegions` gates the whole pass, which is what makes
/// turning it off cost nothing rather than merely hiding drawn geometry.
///
/// ⚠️ A **disabled** region is drawn in the disabled (grey) colour, NOT skipped.
/// This is where the overlay deliberately diverges from the runtime navigation
/// map, which drops disabled regions entirely (`main_loop.rs:1761`). Seeing a
/// greyed-out region is how an author notices the `enabled` flag is off; an
/// overlay that skipped it would render exactly like a region that failed to
/// bake, and those need different fixes.
pub fn navigation_overlay_2d(tree: &SceneTree, overlays: &OverlaySettings) -> NavigationOverlay {
    let mut out = NavigationOverlay::default();
    if !overlays.is_enabled(CanvasOverlay::NavigationRegions) {
        return out;
    }

    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if node.class_name() != "NavigationRegion2D" {
            continue;
        }
        let enabled = region_enabled(node);
        let (face_color, edge_color) = if enabled {
            (FACE_COLOR, EDGE_COLOR)
        } else {
            (FACE_DISABLED_COLOR, EDGE_DISABLED_COLOR)
        };
        // Vertices are stored relative to the region node, so the overlay is
        // offset into world space the same way the navigation map does it.
        let origin = gdscene::node2d::get_position(tree, id);

        for local in navigation_polygons_local(node) {
            let points: Vec<Vector2> = local.iter().map(|&v| v + origin).collect();
            // Edges close the loop: the last point joins back to the first, so a
            // face of N points contributes N segments, not N - 1.
            for w in 0..points.len() {
                out.edges.push(NavEdge {
                    from: points[w],
                    to: points[(w + 1) % points.len()],
                    color: edge_color,
                    region: id,
                });
            }
            out.faces.push(NavFace {
                points,
                color: face_color,
                region: id,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;

    /// An L-shaped region: two indexed polygons over six shared vertices.
    fn l_shaped_region() -> SceneTree {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut region = Node::new("Region", "NavigationRegion2D");
        region.set_property(
            "__navmesh_vertices__",
            Variant::PackedVector2Array(vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(10.0, 0.0),
                Vector2::new(10.0, 10.0),
                Vector2::new(0.0, 10.0),
                Vector2::new(20.0, 0.0),
                Vector2::new(20.0, 10.0),
            ]),
        );
        region.set_property(
            "__navmesh_polygons__",
            Variant::Array(
                vec![
                    Variant::PackedInt32Array(vec![0, 1, 2, 3]),
                    Variant::PackedInt32Array(vec![1, 4, 5, 2]),
                ]
                .into(),
            ),
        );
        tree.add_child(root, region).expect("region attaches");
        tree
    }

    fn overlays_on() -> OverlaySettings {
        let mut o = OverlaySettings::new();
        o.enable(CanvasOverlay::NavigationRegions);
        o
    }

    /// 🔑 THE MONITOR AND THE OVERLAY CANNOT DISAGREE.
    ///
    /// Asserted against `navigation_overlay_2d`'s own output rather than against
    /// the literals 2 and 8. That is the whole reason the counter shares
    /// the shared reader instead of re-reading `__navmesh_polygons__`: if a
    /// second reader ever drifts — on the no-`polygons`-entry fallback, or on
    /// dropping out-of-range indices — this fails, whereas hardcoded numbers
    /// would keep passing while the two views of the scene diverged.
    #[test]
    fn the_geometry_counts_agree_with_what_the_overlay_draws() {
        let tree = l_shaped_region();
        let overlay = navigation_overlay_2d(&tree, &overlays_on());
        let (polygons, edges) = navigation_geometry_counts(&tree);

        assert_eq!(polygons, overlay.faces.len() as u64);
        assert_eq!(edges, overlay.edges.len() as u64);
        // And the absolute values, so a bug that emptied BOTH would not pass.
        assert_eq!(polygons, 2, "an L-shape is two indexed polygons, not one");
        assert_eq!(edges, 8, "two closed quads are eight edges, not six");
    }

    /// 🔑 THE COUNTS REPORT THE SCENE, NOT THE VIEW MENU.
    ///
    /// ⚠️ The overlay is gated on `CanvasOverlay::NavigationPolygons` and
    /// returns nothing when it is off. A monitor sharing that gate would drop to
    /// zero the moment an author merely hid the overlay, which reads as "the
    /// bake was lost" — the opposite of what the number is for.
    #[test]
    fn the_counts_ignore_the_view_toggle_that_gates_the_overlay() {
        let tree = l_shaped_region();
        let mut off = OverlaySettings::new();
        off.disable(CanvasOverlay::NavigationRegions);

        assert!(
            navigation_overlay_2d(&tree, &off).faces.is_empty(),
            "the overlay must be empty with the toggle off, or this test is \
             comparing against nothing"
        );
        assert_eq!(
            navigation_geometry_counts(&tree),
            (2, 8),
            "but the monitor still reports the scene's baked geometry"
        );
    }

    /// 🔑 AN UNBAKED REGION CONTRIBUTES NOTHING, AND SO DOES A DEGENERATE ONE.
    ///
    /// ⚠️ The sub-3-point polygon is the interesting half: it must be skipped
    /// for BOTH counts, exactly as the overlay skips it when drawing. Counting
    /// its two edges while drawing no face would make the monitor report
    /// geometry no one can see.
    #[test]
    fn unbaked_and_degenerate_regions_contribute_nothing() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.add_child(root, Node::new("Unbaked", "NavigationRegion2D"))
            .expect("region attaches");
        assert_eq!(navigation_geometry_counts(&tree), (0, 0));

        let mut degenerate = Node::new("Degenerate", "NavigationRegion2D");
        degenerate.set_property(
            "__navmesh_vertices__",
            Variant::PackedVector2Array(vec![Vector2::new(0.0, 0.0), Vector2::new(1.0, 0.0)]),
        );
        degenerate.set_property(
            "__navmesh_polygons__",
            Variant::Array(vec![Variant::PackedInt32Array(vec![0, 1])].into()),
        );
        tree.add_child(root, degenerate).expect("region attaches");
        assert_eq!(
            navigation_geometry_counts(&tree),
            (0, 0),
            "a two-point polygon is not a face and must contribute no edges"
        );
    }
}
