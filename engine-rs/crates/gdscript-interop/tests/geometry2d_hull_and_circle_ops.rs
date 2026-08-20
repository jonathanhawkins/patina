//! pat-s3rrv.125 — `Geometry2D.convex_hull`, `is_point_in_circle` and
//! `segment_intersects_circle` from GDScript.
//!
//! `call_geometry_2d_static` answered 8 of Godot's 24 methods; these three were
//! among the missing and every call raised `UndefinedFunction`.
//!
//! Every expected value here was derived by simulating Godot's own algorithm
//! (`core/math/geometry_2d.h`) rather than reasoned out by hand — the hull trace
//! in particular is easy to get wrong on paper.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn eval(expr: &str) -> Variant {
    let src = format!("extends Node\nfunc probe():\n\x20   return {expr}\n");
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "probe", &[])
        .unwrap_or_else(|e| panic!("`{expr}` raised: {e:?}"))
}

fn floats(expr: &str) -> f64 {
    match eval(expr) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("`{expr}` must be a number, got {other:?}"),
    }
}

/// A unit square plus an interior point and a point collinear with an edge.
const SQUARE_PLUS_NOISE: &str = "[Vector2(0, 0), Vector2(1, 0), Vector2(1, 1), \
                                 Vector2(0, 1), Vector2(0.5, 0.5), Vector2(0.5, 0)]";

/// 🔑 **The hull is CLOSED** — Godot ends with `H.resize(k)`, keeping the
/// repeated first point, where the textbook monotone chain does `resize(k - 1)`
/// and drops it.
///
/// ⚠️ A caller drawing the outline depends on that closing point: the open form
/// leaves the last edge unclosed, and it changes `size()`. This is the single
/// most likely detail to get wrong when porting the algorithm from memory.
#[test]
fn the_convex_hull_is_closed() {
    let hull = match eval(&format!("Geometry2D.convex_hull({SQUARE_PLUS_NOISE})")) {
        Variant::PackedVector2Array(v) => v,
        other => panic!("convex_hull must return a PackedVector2Array, got {other:?}"),
    };

    assert_eq!(
        hull.len(),
        5,
        "four corners plus the repeated closing point, got {hull:?}"
    );
    assert_eq!(
        (hull[0].x, hull[0].y),
        (hull[4].x, hull[4].y),
        "the first point must be repeated as the last"
    );
}

/// ⚠️ The pop test is `cross <= 0`, not `< 0`, so an interior point AND a point
/// lying on an edge are both discarded — the hull carries only true corners.
#[test]
fn the_hull_drops_interior_and_collinear_points() {
    let hull = match eval(&format!("Geometry2D.convex_hull({SQUARE_PLUS_NOISE})")) {
        Variant::PackedVector2Array(v) => v,
        other => panic!("expected PackedVector2Array, got {other:?}"),
    };
    let has = |x: f32, y: f32| hull.iter().any(|p| p.x == x && p.y == y);

    assert!(!has(0.5, 0.5), "the interior point must not be on the hull");
    assert!(
        !has(0.5, 0.0),
        "a point collinear with an edge is not a corner — `<= 0` discards it"
    );
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        assert!(has(x, y), "corner ({x}, {y}) must survive");
    }
}

/// 🔑 The miss value is **-1.0, not 0.0** — `0.0` is a legitimate hit exactly at
/// `from`, so a caller testing `if t > 0` would drop the grazing case and one
/// testing `if t != 0` would read every miss as a hit.
#[test]
fn a_missed_segment_reports_minus_one() {
    let t = floats(
        "Geometry2D.segment_intersects_circle(Vector2(0, 5), Vector2(4, 5), Vector2(2, 0), 1)",
    );
    assert!(
        (t + 1.0).abs() < 1e-6,
        "a miss must be exactly -1.0, got {t}"
    );
}

/// The straightforward crossing: a segment along x from 0 to 4 through a unit
/// circle at (2, 0) first meets it at x = 1, a quarter of the way along.
#[test]
fn a_crossing_segment_reports_the_first_intersection() {
    let t = floats(
        "Geometry2D.segment_intersects_circle(Vector2(0, 0), Vector2(4, 0), Vector2(2, 0), 1)",
    );
    assert!((t - 0.25).abs() < 1e-6, "expected 0.25, got {t}");
}

/// 🔑 A segment that **starts inside** the circle still reports a hit.
///
/// ⚠️ Its near root is behind `from` (negative) and only the far root lies in
/// range, so an implementation that returns the smaller root unconditionally —
/// which upstream notes is valid only when the segment is known to start
/// outside — reports a MISS for every segment fired from within the circle.
/// That version returns -1.0 here, so this case discriminates.
#[test]
fn a_segment_starting_inside_the_circle_still_hits() {
    let t = floats(
        "Geometry2D.segment_intersects_circle(Vector2(2, 0), Vector2(4, 0), Vector2(2, 0), 1)",
    );
    assert!(
        (t - 0.5).abs() < 1e-6,
        "a segment from the centre exits at half its length, got {t}"
    );
}

/// The comparison is squared and inclusive, so a point exactly on the
/// circumference counts as inside.
#[test]
fn is_point_in_circle_includes_the_circumference() {
    assert_eq!(
        eval("Geometry2D.is_point_in_circle(Vector2(0, 0), Vector2(0, 0), 1)"),
        Variant::Bool(true),
        "the centre is inside"
    );
    assert_eq!(
        eval("Geometry2D.is_point_in_circle(Vector2(1, 0), Vector2(0, 0), 1)"),
        Variant::Bool(true),
        "a point exactly on the circumference is inside (`<=`)"
    );
    assert_eq!(
        eval("Geometry2D.is_point_in_circle(Vector2(2, 0), Vector2(0, 0), 1)"),
        Variant::Bool(false),
        "and a point beyond it is not — without this the predicate could be \
         returning true unconditionally"
    );
}
