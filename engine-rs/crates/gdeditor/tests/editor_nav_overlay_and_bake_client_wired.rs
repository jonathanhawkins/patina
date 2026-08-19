//! pat-s3rrv.82 acceptance — the navigation overlay draws, and the served HTML
//! can reach the bake route.
//!
//! Godot draws a navigation region as **filled faces plus outlined edges**, in
//! the colours declared at `servers/navigation_2d/navigation_server_2d.cpp:231`.
//! Patina had a navigation overlay toggle that nothing read, and no renderer
//! referenced navigation geometry at all — so the View menu item was inert.
//!
//! ⚠️ The gate is `viewport_2d::CanvasOverlay::NavigationRegions`, the overlay
//! set the viewport actually stores (`Viewport2D.overlays`) and already uses to
//! drive `OriginCross`. There is a SECOND, identically-named `CanvasOverlay` in
//! `canvas_overlays.rs` carrying a `NavigationPolygons` variant; nothing stores
//! an instance of its container, so an overlay gated on it can never be switched
//! on by the running editor. The two enums have different variant sets and are
//! not interchangeable — the `use` line is the only thing distinguishing them at
//! a call site, which is exactly how the wrong one gets picked.

use gdcore::math::Vector2;
use gdeditor::editor_ui::EDITOR_HTML;
use gdeditor::navigation_overlay::{
    navigation_overlay_2d, EDGE_COLOR, EDGE_DISABLED_COLOR, FACE_COLOR, FACE_DISABLED_COLOR,
};
use gdeditor::viewport_2d::{CanvasOverlay, OverlaySettings};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// An L-shaped region: two polygons over six shared vertices.
///
/// ⚠️ Two polygons on purpose. A region is a SET of indexed polygons, and an
/// implementation that read the flat vertex array as one convex blob would draw
/// a filled quad straight across the L's notch — geometry the region does not
/// have. A single-polygon fixture cannot tell the two apart.
fn l_shaped_region(enabled: bool, position: Vector2) -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut region = Node::new("Region", "NavigationRegion2D");
    region.set_property("position", Variant::Vector2(position));
    if !enabled {
        region.set_property("enabled", Variant::Bool(false));
    }
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
        Variant::Array(vec![
            Variant::PackedInt32Array(vec![0, 1, 2, 3]),
            Variant::PackedInt32Array(vec![1, 4, 5, 2]),
        ]),
    );
    tree.add_child(root, region).expect("region attaches");
    tree
}

fn overlays_with_navigation(enabled: bool) -> OverlaySettings {
    let mut o = OverlaySettings::new();
    if enabled {
        o.enable(CanvasOverlay::NavigationRegions);
    } else {
        o.disable(CanvasOverlay::NavigationRegions);
    }
    o
}

/// **The acceptance test.**
#[test]
fn editor_nav_overlay_and_bake_client_wired() {
    // --- The served HTML can reach the bake route at all. Before this bead the
    //     route existed in the dispatch table with zero references in the page.
    assert!(
        EDITOR_HTML.contains("/api/navigation/bake"),
        "the served editor page must invoke the bake route; a route nothing \
         calls is unreachable from the UI",
    );

    let tree = l_shaped_region(true, Vector2::new(0.0, 0.0));
    let overlay = navigation_overlay_2d(&tree, &overlays_with_navigation(true));

    // --- Faces. Two polygons, NOT one merged blob.
    assert_eq!(
        overlay.faces.len(),
        2,
        "an L-shaped region is two indexed polygons; one face here means the \
         vertex array was read as a single convex blob, which fills the notch",
    );
    assert!(
        overlay.faces.iter().all(|f| f.points.len() == 4),
        "each face keeps its own four points, got {:?}",
        overlay
            .faces
            .iter()
            .map(|f| f.points.len())
            .collect::<Vec<_>>(),
    );

    // --- Edges close the loop: a 4-point face contributes 4 segments, not 3.
    assert_eq!(
        overlay.edges.len(),
        8,
        "two closed quads are eight edge segments; six would mean the last \
         point never joins back to the first and each face is drawn open",
    );
    assert!(
        overlay.faces.iter().all(|f| f.color == FACE_COLOR)
            && overlay.edges.iter().all(|e| e.color == EDGE_COLOR),
        "an enabled region uses Godot's cyan geometry colours",
    );
}

/// 🔑 The toggle gates the whole pass.
#[test]
fn navigation_overlay_is_empty_while_the_toggle_is_off() {
    let tree = l_shaped_region(true, Vector2::new(0.0, 0.0));
    let overlay = navigation_overlay_2d(&tree, &overlays_with_navigation(false));

    assert!(
        overlay.is_empty(),
        "with the View-menu overlay off the draw list must be empty, not merely \
         hidden; got {} faces and {} edges",
        overlay.faces.len(),
        overlay.edges.len(),
    );
}

/// 🔑 A DISABLED region is greyed out, not omitted.
///
/// ⚠️ This is where the overlay deliberately diverges from the runtime
/// navigation map, which drops disabled regions entirely. An overlay that
/// reused that rule would render a disabled region identically to one that
/// failed to bake — and those need opposite fixes. The colour is the assertion;
/// "it still draws" alone would not catch a region drawn as if enabled.
#[test]
fn disabled_region_is_drawn_in_the_disabled_colour() {
    let tree = l_shaped_region(false, Vector2::new(0.0, 0.0));
    let overlay = navigation_overlay_2d(&tree, &overlays_with_navigation(true));

    assert_eq!(
        overlay.faces.len(),
        2,
        "a disabled region must still be DRAWN — greyed out is how an author \
         sees the `enabled` flag is off, rather than the region vanishing",
    );
    assert!(
        overlay.faces.iter().all(|f| f.color == FACE_DISABLED_COLOR),
        "…in the disabled face colour, got {:?}",
        overlay.faces.first().map(|f| f.color),
    );
    assert!(
        overlay.edges.iter().all(|e| e.color == EDGE_DISABLED_COLOR),
        "…and the disabled edge colour",
    );
}

/// 🔑 Geometry is emitted in WORLD space, offset by the region's position.
///
/// ⚠️ The fixture is deliberately off-origin. At position (0,0) the local and
/// world coordinates coincide, so an implementation that forgot the offset
/// entirely would pass every other test in this file.
#[test]
fn overlay_geometry_is_offset_into_world_space() {
    let origin = Vector2::new(100.0, 40.0);
    let tree = l_shaped_region(true, origin);
    let overlay = navigation_overlay_2d(&tree, &overlays_with_navigation(true));

    let first = &overlay.faces[0].points[0];
    assert_eq!(
        *first, origin,
        "local vertex (0,0) on a region at {origin:?} must land at {origin:?} \
         in world space; (0,0) here means the region's position was ignored",
    );
    assert!(
        overlay
            .faces
            .iter()
            .flat_map(|f| f.points.iter())
            .all(|p| p.x >= origin.x && p.y >= origin.y),
        "every point shifts by the region origin, not just the first",
    );
}
