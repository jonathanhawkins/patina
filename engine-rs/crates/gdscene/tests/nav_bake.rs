//! pat-0q2kx.186 (partial) — `NavigationPolygon` outline baking carves
//! obstruction outlines out of the traversable area.
//!
//! Godot ground truth: `modules/navigation_2d/2d/nav_mesh_generator_2d.cpp`
//! parses traversable and obstruction outlines, carves the obstructions, and
//! partitions the remainder; `make_polygons_from_outlines` is the manual entry.
//!
//! Patina had **nothing** here — `grep make_polygons_from_outlines` across the
//! workspace matched zero, so a NavigationPolygon holding outlines contributed
//! no navigable area whatsoever.
//!
//! ⚠️ This does NOT satisfy the whole acceptance. Delivered: the standalone
//! outline→polygon bake with carving, plus ear-clip triangulation. NOT
//! delivered: `NavigationRegion2D.bake_navigation_polygon` dispatch, the
//! `bake_finished` signal, source-geometry parsing from scene nodes, and
//! feeding the baked polygons into the live runtime map so an agent paths
//! around the hole. See the completion report.
//!
//! NOTE ON THE FILE NAME: the acceptance names
//! `navigation_polygon_bake_from_outlines_carves`; with `-p gdscene --test`
//! that command is 79 characters and the verifier truncates at 58. The
//! acceptance's name is kept on the test function.

use gdcore::math::Vector2;
use gdscene::nav_polygon::{triangulate, NavigationPolygon};

/// Sample points are deliberately cell CENTRES (x.5): a point on a grid corner
/// sits on the boundary of four cells, where any point-in-polygon test is
/// ambiguous and could report false for all of them.
fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Vector2> {
    vec![
        Vector2::new(x0, y0),
        Vector2::new(x1, y0),
        Vector2::new(x1, y1),
        Vector2::new(x0, y1),
    ]
}

/// **The acceptance.** A room with a hole bakes to polygons that exclude the
/// hole.
#[test]
fn navigation_polygon_bake_from_outlines_carves() {
    let mut nav = NavigationPolygon::new();
    nav.add_outline(rect(0.0, 0.0, 10.0, 10.0)); // traversable room
    nav.add_outline(rect(4.0, 4.0, 6.0, 6.0)); // obstruction to carve

    let count = nav.make_polygons_from_outlines();
    assert!(count > 0, "the room must bake to some navigable polygons");

    // The carved centre is NOT navigable — the property the whole bead exists
    // for, and the one a centroid-rejection bake silently fails.
    assert!(
        !nav.contains_point(Vector2::new(4.5, 4.5)),
        "the carved hole must not be covered by any polygon"
    );

    // The surrounding ring still is, on all four sides — so the carve removed
    // the hole rather than the whole room.
    for p in [
        Vector2::new(1.5, 1.5),
        Vector2::new(8.5, 1.5),
        Vector2::new(1.5, 8.5),
        Vector2::new(8.5, 8.5),
        Vector2::new(5.5, 1.5),
    ] {
        assert!(
            nav.contains_point(p),
            "{p:?} is outside the hole and must remain navigable"
        );
    }

    // No baked polygon may overlap the hole's centre.
    for poly in nav.polygons() {
        assert!(
            poly.len() >= 3,
            "every baked polygon must be a real polygon, got {poly:?}"
        );
    }
}

/// Without an obstruction the whole room is navigable, so the carve above is
/// attributable to the hole rather than to the bake dropping area generally.
#[test]
fn without_an_obstruction_the_centre_is_navigable() {
    let mut nav = NavigationPolygon::new();
    nav.add_outline(rect(0.0, 0.0, 10.0, 10.0));
    assert!(nav.make_polygons_from_outlines() > 0);
    assert!(
        nav.contains_point(Vector2::new(4.5, 4.5)),
        "with no hole that same point must be navigable — otherwise the carve test \
         proves nothing"
    );
}

/// A second outline that does NOT lie inside the first is a separate region,
/// not a hole.
#[test]
fn a_disjoint_outline_is_not_treated_as_a_hole() {
    let mut nav = NavigationPolygon::new();
    nav.add_outline(rect(0.0, 0.0, 10.0, 10.0));
    nav.add_outline(rect(20.0, 20.0, 30.0, 30.0)); // elsewhere entirely
    nav.make_polygons_from_outlines();
    assert!(
        nav.contains_point(Vector2::new(4.5, 4.5)),
        "a disjoint outline must not carve the first room"
    );
}

/// Outline bookkeeping matches Godot's NavigationPolygon surface.
#[test]
fn outline_accessors_and_clear() {
    let mut nav = NavigationPolygon::new();
    assert_eq!(nav.outline_count(), 0);
    assert_eq!(
        nav.make_polygons_from_outlines(),
        0,
        "baking with no outlines produces nothing rather than panicking"
    );

    nav.add_outline(rect(0.0, 0.0, 4.0, 4.0));
    assert_eq!(nav.outline_count(), 1);
    assert_eq!(nav.get_outline(0).map(|o| o.len()), Some(4));
    assert!(nav.get_outline(9).is_none(), "out of range reads as None");

    nav.make_polygons_from_outlines();
    assert!(!nav.polygons().is_empty());

    nav.clear_outlines();
    assert_eq!(nav.outline_count(), 0);
    assert!(
        nav.polygons().is_empty(),
        "clearing outlines must drop the stale baked polygons too"
    );
}

/// The ear-clip triangulator handles convex and concave outlines.
#[test]
fn triangulate_convex_and_concave() {
    // A quad is two triangles.
    let tris = triangulate(&rect(0.0, 0.0, 10.0, 10.0));
    assert_eq!(tris.len(), 2, "a quad triangulates into 2 triangles");

    // An L-shape (6 vertices, one reflex corner) is 4 triangles — n-2 for any
    // simple polygon. A triangulator that mishandles the reflex vertex returns
    // fewer.
    let l_shape = vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(6.0, 0.0),
        Vector2::new(6.0, 2.0),
        Vector2::new(2.0, 2.0),
        Vector2::new(2.0, 6.0),
        Vector2::new(0.0, 6.0),
    ];
    assert_eq!(
        triangulate(&l_shape).len(),
        4,
        "an n-vertex simple polygon triangulates into n-2 triangles"
    );

    // Reversed winding must give the same count — the triangulator normalises
    // orientation, and a clockwise outline would otherwise clip nothing.
    let mut reversed = l_shape.clone();
    reversed.reverse();
    assert_eq!(
        triangulate(&reversed).len(),
        4,
        "winding must not change the triangulation"
    );

    // Degenerate inputs produce nothing rather than panicking.
    assert!(triangulate(&[]).is_empty());
    assert!(triangulate(&[Vector2::ZERO, Vector2::new(1.0, 1.0)]).is_empty());
}
