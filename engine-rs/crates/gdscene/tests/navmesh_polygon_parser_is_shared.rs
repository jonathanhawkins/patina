//! pat-s3rrv.82: the navmesh polygon-index parser has ONE implementation.
//!
//! # Why this test exists
//!
//! `gdscene::main_loop` carried a byte-identical copy of the index parser that
//! `gdscene::navigation::navmesh_polygons_local` used internally. Two
//! hand-maintained copies of one parser is the shape that drifts silently: the
//! editor overlay reads one, the runtime navmesh reads the other, and the first
//! symptom is a region that is walkable in play and empty in the overlay — or
//! the reverse — with nothing pointing at the cause.
//!
//! The copy is gone; `main_loop` now calls the `navigation` one. This pins the
//! edge cases that were duplicated, because they are the parts a re-divergence
//! would get wrong: they are not obvious from the signature and they are exactly
//! what a second implementation would "simplify".
//!
//! ⚠️ Driven through `navmesh_polygons_local`, which is the public surface. The
//! shared index parser is `pub(crate)` and deliberately not exported — a test
//! reaching past the public function would pin the private shape and make the
//! next refactor harder rather than safer.

use gdcore::math::Vector2;
use gdscene::navigation::navmesh_polygons_local;
use gdscene::node::Node;
use gdvariant::Variant;

/// A NavigationRegion2D-ish node carrying a baked vertex pool.
fn region(verts: &[(f32, f32)], polys: Option<Vec<Vec<i32>>>) -> Node {
    let mut n = Node::new("Region", "NavigationRegion2D");
    n.set_property(
        "__navmesh_vertices__",
        Variant::PackedVector2Array(verts.iter().map(|(x, y)| Vector2::new(*x, *y)).collect()),
    );
    if let Some(polys) = polys {
        n.set_property(
            "__navmesh_polygons__",
            Variant::Array(
                (polys
                    .into_iter()
                    .map(Variant::PackedInt32Array)
                    .collect::<Vec<_>>())
                .into(),
            ),
        );
    }
    n
}

const SQUARE: &[(f32, f32)] = &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];

// ===========================================================================

/// An explicit polygon list is resolved against the vertex pool.
#[test]
fn navmesh_polygon_parser_is_shared() {
    let node = region(SQUARE, Some(vec![vec![0, 1, 2], vec![0, 2, 3]]));
    let polys = navmesh_polygons_local(&node);

    assert_eq!(polys.len(), 2, "both polygons survive: {polys:?}");
    assert_eq!(
        polys[0],
        vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(10.0, 10.0)
        ],
        "indices resolve to the pooled vertices, in the order given"
    );
}

/// **No polygon list at all means one polygon over the whole pool.**
///
/// A region baked from a single outline has no `__navmesh_polygons__`. Reading
/// that absence as "no polygons" is the tempting simplification and it makes an
/// entire baked region silently unwalkable.
#[test]
fn navmesh_polygon_parser_is_shared_absent_list_means_one_polygon() {
    let polys = navmesh_polygons_local(&region(SQUARE, None));
    assert_eq!(
        polys.len(),
        1,
        "an absent polygon list is one polygon over every vertex: {polys:?}"
    );
    assert_eq!(polys[0].len(), 4, "…using the whole pool: {polys:?}");

    // The paired negative: fewer than three vertices cannot form a polygon, so
    // the fallback must NOT fire. Without this, "always synthesise one" passes
    // the assertion above.
    let thin = region(&[(0.0, 0.0), (1.0, 1.0)], None);
    assert!(
        navmesh_polygons_local(&thin).is_empty(),
        "two vertices cannot make a polygon"
    );
}

/// An index outside the vertex pool is DROPPED, not clamped, and never panics.
///
/// This is the case that decides between a lost corner and a crash: indexing the
/// pool with an out-of-range index panics, and a `.tscn` can legally contain
/// one.
#[test]
fn navmesh_polygon_parser_is_shared_drops_out_of_range_indices() {
    // 99 does not exist in a 4-vertex pool; 0,1,2 do.
    let node = region(SQUARE, Some(vec![vec![0, 1, 2, 99]]));
    let polys = navmesh_polygons_local(&node);

    assert_eq!(
        polys.len(),
        1,
        "the polygon survives its bad index: {polys:?}"
    );
    assert_eq!(
        polys[0].len(),
        3,
        "the out-of-range index is dropped, leaving the three real corners: {polys:?}"
    );
    assert!(
        !polys[0].contains(&Vector2::new(0.0, 10.0)),
        "dropping must not silently substitute another vertex: {polys:?}"
    );
}

/// A polygon left with fewer than three points after filtering is dropped.
///
/// Godot will not draw or path through a degenerate polygon, and a caller that
/// assumes every returned polygon is drawable would index `[2]` on a 2-point
/// one.
#[test]
fn navmesh_polygon_parser_is_shared_drops_degenerate_polygons() {
    let node = region(SQUARE, Some(vec![vec![0, 1], vec![0, 1, 2]]));
    let polys = navmesh_polygons_local(&node);

    assert_eq!(
        polys.len(),
        1,
        "the two-point polygon is dropped and the triangle kept: {polys:?}"
    );
    assert_eq!(polys[0].len(), 3, "and it is the triangle: {polys:?}");
}

/// Points come back in LOCAL space — the region's own position is not applied.
///
/// Every caller adds the position itself; handing out pre-offset points is how a
/// caller ends up applying it twice, which draws the overlay at double the
/// region's offset and is easy to misread as a camera bug.
#[test]
fn navmesh_polygon_parser_is_shared_returns_local_space() {
    let mut node = region(SQUARE, Some(vec![vec![0, 1, 2]]));
    node.set_property("position", Variant::Vector2(Vector2::new(500.0, 500.0)));

    let polys = navmesh_polygons_local(&node);
    assert_eq!(
        polys[0][0],
        Vector2::new(0.0, 0.0),
        "the region's position must not be baked into the returned points"
    );
}
