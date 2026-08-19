//! pat-s3rrv.19 — the AStar methods the bead lists as missing from the engine.
//!
//! `get_available_point_id`, `reserve_space`, `get_point_capacity`,
//! `get_closest_position_in_segment` and `AStarGrid2D::is_dirty` did not exist
//! at all — `grep "pub fn get_available_point_id" crates/gdscene/src/astar.rs`
//! returned nothing — and `AStarGrid2D::update()` was a literal `{}`, so the
//! documented "change the region, then call `update()`" sequence had no
//! observable effect.
//!
//! These test the ENGINE directly. The separate script-binding defect this bead
//! also covers is unrelated to any of it: the pathfinder works, and it is the
//! dispatch between GDScript and this code that is broken.

use gdcore::math::{Vector2, Vector2i, Vector3};
use gdscene::astar::{AStar2D, AStar3D, AStarGrid2D};

fn approx2(a: Vector2, b: Vector2) -> bool {
    (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4
}

/// **The acceptance test**: ids, capacity and the segment query.
#[test]
fn astar_id_capacity_and_segment_queries() {
    let mut a = AStar2D::new();
    assert_eq!(
        a.get_available_point_id(),
        0,
        "an empty graph's first free id is 0",
    );

    a.add_point(0, Vector2::new(0.0, 0.0), 1.0);
    a.add_point(1, Vector2::new(10.0, 0.0), 1.0);
    assert_eq!(
        a.get_available_point_id(),
        2,
        "with 0 and 1 taken the next free id is 2 — this is what \
         `a.add_point(a.get_available_point_id(), pos)` relies on",
    );

    // A hole is reusable. ⚠️ Godot caches a `last_free_id` that only moves up,
    // so a Godot build may strand this id; the documented contract is "the next
    // available ID with no point associated to it", which is what this answers.
    a.remove_point(0);
    assert_eq!(a.get_available_point_id(), 0, "the freed id is available");

    // -- capacity is a hint, not graph state -------------------------------
    let mut b = AStar2D::new();
    b.reserve_space(64);
    assert!(
        b.get_point_capacity() >= 64,
        "reserve_space must actually reserve, got {}",
        b.get_point_capacity(),
    );
    assert_eq!(
        b.get_point_count(),
        0,
        "reserving must not invent points — it is an allocation hint and \
         changes no observable graph state",
    );
}

/// The closest position lies ON the segment, not on its infinite line
/// (pat-s3rrv.19).
///
/// Godot projects onto each segment and clamps; an unclamped projection returns
/// points off the end of the graph, which is the natural implementation slip.
#[test]
fn the_closest_position_is_clamped_to_the_segment() {
    let mut a = AStar2D::new();
    a.add_point(0, Vector2::new(0.0, 0.0), 1.0);
    a.add_point(1, Vector2::new(10.0, 0.0), 1.0);
    a.connect_points(0, 1, true);

    // Beside the middle of the segment → projects straight down onto it.
    let mid = a.get_closest_position_in_segment(Vector2::new(5.0, 4.0), Vector2::ZERO);
    assert!(
        approx2(mid, Vector2::new(5.0, 0.0)),
        "expected (5, 0), got {mid:?}",
    );

    // Well past the far end → clamped to the endpoint, NOT extrapolated.
    let past = a.get_closest_position_in_segment(Vector2::new(99.0, 1.0), Vector2::ZERO);
    assert!(
        approx2(past, Vector2::new(10.0, 0.0)),
        "a point beyond the segment must clamp to its end: expected (10, 0), \
         got {past:?} — (99, 0) is the signature of an unclamped projection \
         onto the infinite line",
    );
}

/// A segment with a disabled endpoint is skipped entirely (pat-s3rrv.19).
///
/// ⚠️ Godot checks `from_point->enabled && to_point->enabled`: disabling a
/// point removes its EDGES from consideration, not just the point. Skipping
/// only the disabled endpoint would still return a position on an edge no path
/// can use.
#[test]
fn a_disabled_endpoint_removes_its_whole_segment() {
    let mut a = AStar2D::new();
    a.add_point(0, Vector2::new(0.0, 0.0), 1.0);
    a.add_point(1, Vector2::new(10.0, 0.0), 1.0);
    a.add_point(2, Vector2::new(0.0, 100.0), 1.0);
    a.add_point(3, Vector2::new(10.0, 100.0), 1.0);
    a.connect_points(0, 1, true);
    a.connect_points(2, 3, true);

    // Nearest to the lower segment while both are live.
    let live = a.get_closest_position_in_segment(Vector2::new(5.0, 1.0), Vector2::ZERO);
    assert!(approx2(live, Vector2::new(5.0, 0.0)), "{live:?}");

    // Disable ONE endpoint of that segment: the whole edge drops out, so the
    // far segment wins even though it is 99 units away.
    a.set_point_disabled(1, true);
    let after = a.get_closest_position_in_segment(Vector2::new(5.0, 1.0), Vector2::ZERO);
    assert!(
        approx2(after, Vector2::new(5.0, 100.0)),
        "expected the far segment (5, 100), got {after:?} — (5, 0) means the \
         segment survived its disabled endpoint",
    );

    // No eligible segment at all → the caller's fallback.
    let mut empty = AStar2D::new();
    let none = empty.get_closest_position_in_segment(Vector2::new(1.0, 1.0), Vector2::ZERO);
    assert!(approx2(none, Vector2::ZERO), "{none:?}");
    empty.add_point(0, Vector2::new(0.0, 0.0), 1.0);
    let unconnected = empty.get_closest_position_in_segment(Vector2::new(1.0, 1.0), Vector2::ZERO);
    assert!(
        approx2(unconnected, Vector2::ZERO),
        "a lone point forms no segment: {unconnected:?}",
    );
}

/// `AStarGrid2D::update()` clears the dirty flag its setters raise
/// (pat-s3rrv.19).
///
/// `update()` was `{}`, so `is_dirty()` had nothing to answer and the whole
/// "change the region, then update" contract was unobservable.
#[test]
fn grid_update_clears_the_dirty_flag() {
    let mut g = AStarGrid2D::new();
    assert!(
        g.is_dirty(),
        "a fresh grid has no region yet, so it starts dirty",
    );

    g.set_region(Vector2i::new(0, 0), Vector2i::new(4, 4));
    assert!(g.is_dirty(), "setting the region dirties the grid");
    g.update();
    assert!(!g.is_dirty(), "update() must clear it");

    // Every setter that changes the grid's SHAPE dirties it again.
    g.set_cell_size(Vector2::new(2.0, 2.0));
    assert!(g.is_dirty(), "cell size is a shape change");
    g.update();

    g.set_offset(Vector2::new(1.0, 0.0));
    assert!(g.is_dirty(), "offset is a shape change");
    g.update();
    assert!(!g.is_dirty());
}

/// The 3D graph answers the same segment query (pat-s3rrv.19).
///
/// `AStar3D` shares the graph code through `AStar<V>`, so the projection has to
/// work for both vector widths rather than being written for Vector2 alone.
#[test]
fn the_segment_query_works_in_three_dimensions() {
    let mut a = AStar3D::new();
    a.add_point(0, Vector3::new(0.0, 0.0, 0.0), 1.0);
    a.add_point(1, Vector3::new(0.0, 0.0, 10.0), 1.0);
    a.connect_points(0, 1, true);

    let got = a.get_closest_position_in_segment(Vector3::new(3.0, 0.0, 5.0), Vector3::ZERO);
    assert!(
        (got.x).abs() < 1e-4 && (got.z - 5.0).abs() < 1e-4,
        "expected (0, 0, 5), got {got:?}",
    );
}
