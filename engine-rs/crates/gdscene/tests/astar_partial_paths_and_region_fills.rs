//! pat-0q2kx.185 (slice 1) — `allow_partial` paths and AStarGrid2D region fills.
//!
//! Godot 4's `AStar2D.get_id_path(from, to, allow_partial)` returns the route to
//! the closest reachable point when the goal is walled off, instead of nothing —
//! which is what lets an agent make progress toward an unreachable target rather
//! than standing still. `AStarGrid2D` grows `fill_solid_region` /
//! `fill_weight_scale_region` so carving a wall is one call instead of an
//! O(area) script loop.
//!
//! Neither existed in `astar.rs`: `allow_partial`, `fill_solid_region` and
//! `fill_weight_scale_region` were all zero hits.

use gdcore::math::{Vector2, Vector2i};
use gdscene::astar::{AStar2D, AStarGrid2D};

/// Two connected points, plus an isolated third the search can never reach.
fn split_graph() -> AStar2D {
    let mut a = AStar2D::new();
    a.add_point(1, Vector2::new(0.0, 0.0), 1.0);
    a.add_point(2, Vector2::new(10.0, 0.0), 1.0);
    // Nearer the goal than point 1, so "closest reached" is unambiguous.
    a.add_point(3, Vector2::new(20.0, 0.0), 1.0);
    // 4 is the goal and is connected to NOTHING.
    a.add_point(4, Vector2::new(30.0, 0.0), 1.0);
    a.connect_points(1, 2, true);
    a.connect_points(2, 3, true);
    a
}

/// **Slice acceptance.** A disconnected goal yields nothing without
/// `allow_partial`, and the reachable prefix with it.
#[test]
fn allow_partial_returns_the_reachable_prefix() {
    let a = split_graph();

    assert!(
        a.get_id_path(1, 4).is_empty(),
        "an unreachable goal is still no path by default",
    );

    let partial = a.get_id_path_partial(1, 4, true);
    assert_eq!(
        partial,
        vec![1, 2, 3],
        "allow_partial walks as close to the goal as the graph allows",
    );

    // The reachable case is unaffected — a partial path must never be returned
    // when a real one exists, or callers can't tell success from best-effort.
    assert_eq!(
        a.get_id_path_partial(1, 3, true),
        vec![1, 2, 3],
        "a reachable goal returns the true path, not a prefix",
    );
    assert_eq!(
        a.get_id_path(1, 3),
        vec![1, 2, 3],
        "…same as without the flag"
    );
}

/// A disabled endpoint means "no path", not "get as close as you can".
#[test]
fn allow_partial_still_respects_disabled_endpoints() {
    let mut a = split_graph();
    a.set_point_disabled(4, true);

    assert!(
        a.get_id_path_partial(1, 4, true).is_empty(),
        "a disabled goal yields nothing even with allow_partial",
    );

    let mut b = split_graph();
    b.set_point_disabled(1, true);
    assert!(
        b.get_id_path_partial(1, 3, true).is_empty(),
        "a disabled start yields nothing even with allow_partial",
    );
}

/// A start with nowhere better to go reports itself rather than an empty path.
#[test]
fn allow_partial_from_an_isolated_start_returns_just_the_start() {
    let mut a = AStar2D::new();
    a.add_point(1, Vector2::new(0.0, 0.0), 1.0);
    a.add_point(2, Vector2::new(5.0, 0.0), 1.0);
    // No connection at all.

    assert_eq!(
        a.get_id_path_partial(1, 2, true),
        vec![1],
        "you are already as close as you can get",
    );
}

/// `fill_solid_region` carves a wall, and the path routes around it.
#[test]
fn fill_solid_region_carves_a_wall_the_path_avoids() {
    let mut g = AStarGrid2D::new();
    g.set_region(Vector2i::new(0, 0), Vector2i::new(5, 5));

    // A vertical wall at x = 2 spanning rows 0..4, leaving row 4 open.
    let filled = g.fill_solid_region(Vector2i::new(2, 0), Vector2i::new(1, 4), true);
    assert_eq!(filled, 4, "four cells were carved");
    for y in 0..4 {
        assert!(
            g.is_point_solid(Vector2i::new(2, y)),
            "cell (2,{y}) must be solid",
        );
    }
    assert!(
        !g.is_point_solid(Vector2i::new(2, 4)),
        "the gap at (2,4) must stay open",
    );

    // The path must detour through the gap rather than crossing the wall.
    let path = g.get_id_path(Vector2i::new(0, 0), Vector2i::new(4, 0));
    assert!(!path.is_empty(), "a route around the wall exists");
    for step in &path {
        assert!(
            !(step.x == 2 && step.y < 4),
            "the path must not cross the wall, got {path:?}",
        );
    }

    // Clearing the region again restores the direct route.
    assert_eq!(
        g.fill_solid_region(Vector2i::new(2, 0), Vector2i::new(1, 4), false),
        4
    );
    assert!(!g.is_point_solid(Vector2i::new(2, 0)));
}

/// Fills clamp to the grid and reject degenerate input instead of panicking.
#[test]
fn region_fills_are_bounds_safe() {
    let mut g = AStarGrid2D::new();
    g.set_region(Vector2i::new(0, 0), Vector2i::new(3, 3));

    // Overhanging rectangle: only the in-bounds part is filled.
    let filled = g.fill_solid_region(Vector2i::new(1, 1), Vector2i::new(10, 10), true);
    assert_eq!(
        filled, 4,
        "only the 2x2 overlap with the 3x3 grid is filled"
    );

    // Entirely off-grid, and degenerate extents.
    assert_eq!(
        g.fill_solid_region(Vector2i::new(50, 50), Vector2i::new(2, 2), true),
        0,
        "an off-grid rectangle changes nothing",
    );
    assert_eq!(
        g.fill_solid_region(Vector2i::new(0, 0), Vector2i::new(0, 5), true),
        0,
        "a zero extent fills nothing",
    );

    // A negative weight is rejected outright — a negative edge cost would let
    // the search chase an ever-cheaper path.
    assert_eq!(
        g.fill_weight_scale_region(Vector2i::new(0, 0), Vector2i::new(2, 2), -1.0),
        0,
        "negative weights are refused",
    );
    assert_eq!(
        g.fill_weight_scale_region(Vector2i::new(0, 0), Vector2i::new(2, 2), 3.0),
        4,
        "a valid weight fills the rectangle",
    );
}
