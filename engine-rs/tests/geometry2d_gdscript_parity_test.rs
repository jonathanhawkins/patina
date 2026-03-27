//! pat-hc6g: Geometry2D singleton GDScript parity — newly exposed helpers.
//!
//! Validates that the newly added Geometry2D methods are reachable from
//! GDScript parity fixtures via the interpreter's singleton dispatch:
//!
//! 1. `line_intersects_line` — infinite line intersection
//! 2. `get_closest_point_to_segment_unclamped` — unclamped projection
//! 3. `point_is_inside_triangle` — triangle containment test
//! 4. `is_polygon_clockwise` — winding order detection
//!
//! Also validates the documented stub boundary: methods that exist in
//! Godot's Geometry2D but are not yet implemented return clear errors.
//!
//! Acceptance: minimal bindings plus tests; documented stub boundary.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

// ===========================================================================
// Helpers
// ===========================================================================

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    let result = interp.run(src).expect("interpreter error");
    result.return_value.unwrap_or(Variant::Nil)
}

fn assert_near(a: f32, b: f32, eps: f32, msg: &str) {
    assert!(
        (a - b).abs() < eps,
        "{msg}: expected {b}, got {a} (diff={})",
        (a - b).abs()
    );
}

fn expect_vec2(v: &Variant) -> gdcore::math::Vector2 {
    match v {
        Variant::Vector2(v) => *v,
        other => panic!("expected Vector2, got {other:?}"),
    }
}

// ===========================================================================
// 1. line_intersects_line — GDScript reachability
// ===========================================================================

#[test]
fn gdscript_line_intersects_line_hit() {
    // Horizontal line y=0 intersects vertical line x=5 at (5, 0).
    let result = run_val(
        "return Geometry2D.line_intersects_line(\
           Vector2(0, 0), Vector2(1, 0), \
           Vector2(5, -10), Vector2(0, 1))\n",
    );
    let hit = expect_vec2(&result);
    assert_near(hit.x, 5.0, 1e-4, "line_hit.x");
    assert_near(hit.y, 0.0, 1e-4, "line_hit.y");
}

#[test]
fn gdscript_line_intersects_line_diagonal() {
    // Line A: from (0,0) dir (1,1), Line B: from (10,0) dir (-1,1)
    // They intersect at (5, 5).
    let result = run_val(
        "return Geometry2D.line_intersects_line(\
           Vector2(0, 0), Vector2(1, 1), \
           Vector2(10, 0), Vector2(-1, 1))\n",
    );
    let hit = expect_vec2(&result);
    assert_near(hit.x, 5.0, 1e-4, "diag_hit.x");
    assert_near(hit.y, 5.0, 1e-4, "diag_hit.y");
}

#[test]
fn gdscript_line_intersects_line_parallel_returns_nil() {
    // Two parallel horizontal lines.
    let result = run_val(
        "return Geometry2D.line_intersects_line(\
           Vector2(0, 0), Vector2(1, 0), \
           Vector2(0, 5), Vector2(1, 0))\n",
    );
    assert_eq!(result, Variant::Nil, "parallel lines should return nil");
}

#[test]
fn gdscript_line_intersects_line_wrong_arity() {
    let mut interp = Interpreter::new();
    let result = interp.run(
        "return Geometry2D.line_intersects_line(Vector2(0, 0), Vector2(1, 0))\n",
    );
    assert!(result.is_err(), "line_intersects_line with 2 args should error");
}

// ===========================================================================
// 2. get_closest_point_to_segment_unclamped — GDScript reachability
// ===========================================================================

#[test]
fn gdscript_closest_unclamped_projection() {
    // Point above midpoint of horizontal segment.
    let result = run_val(
        "return Geometry2D.get_closest_point_to_segment_unclamped(\
           Vector2(5, 10), Vector2(0, 0), Vector2(10, 0))\n",
    );
    let p = expect_vec2(&result);
    assert_near(p.x, 5.0, 1e-4, "unclamped.x");
    assert_near(p.y, 0.0, 1e-4, "unclamped.y");
}

#[test]
fn gdscript_closest_unclamped_extends_beyond_segment() {
    // Point far left of a horizontal segment (0,0)-(10,0).
    // Clamped would return (0,0); unclamped should return (-10, 0).
    let result = run_val(
        "return Geometry2D.get_closest_point_to_segment_unclamped(\
           Vector2(-10, 5), Vector2(0, 0), Vector2(10, 0))\n",
    );
    let p = expect_vec2(&result);
    assert_near(p.x, -10.0, 1e-4, "unclamped extends left");
    assert_near(p.y, 0.0, 1e-4, "unclamped.y");
}

#[test]
fn gdscript_closest_unclamped_extends_beyond_end() {
    // Point far right of horizontal segment.
    let result = run_val(
        "return Geometry2D.get_closest_point_to_segment_unclamped(\
           Vector2(20, 3), Vector2(0, 0), Vector2(10, 0))\n",
    );
    let p = expect_vec2(&result);
    assert_near(p.x, 20.0, 1e-4, "unclamped extends right");
    assert_near(p.y, 0.0, 1e-4, "unclamped.y");
}

#[test]
fn gdscript_closest_unclamped_vs_clamped_differ() {
    // Verify unclamped and clamped give different results for a point
    // outside the segment bounds.
    let clamped = run_val(
        "return Geometry2D.get_closest_point_to_segment(\
           Vector2(-10, 0), Vector2(0, 0), Vector2(10, 0))\n",
    );
    let unclamped = run_val(
        "return Geometry2D.get_closest_point_to_segment_unclamped(\
           Vector2(-10, 0), Vector2(0, 0), Vector2(10, 0))\n",
    );
    let c = expect_vec2(&clamped);
    let u = expect_vec2(&unclamped);
    // Clamped: (0, 0), Unclamped: (-10, 0)
    assert_near(c.x, 0.0, 1e-4, "clamped.x");
    assert_near(u.x, -10.0, 1e-4, "unclamped.x");
    assert!((c.x - u.x).abs() > 1.0, "clamped and unclamped should differ");
}

#[test]
fn gdscript_closest_unclamped_wrong_arity() {
    let mut interp = Interpreter::new();
    let result = interp.run(
        "return Geometry2D.get_closest_point_to_segment_unclamped(Vector2(0, 0))\n",
    );
    assert!(result.is_err(), "unclamped with 1 arg should error");
}

// ===========================================================================
// 3. point_is_inside_triangle — GDScript reachability
// ===========================================================================

#[test]
fn gdscript_point_inside_triangle_true() {
    let result = run_val(
        "return Geometry2D.point_is_inside_triangle(\
           Vector2(5, 5), Vector2(0, 0), Vector2(10, 0), Vector2(5, 10))\n",
    );
    assert_eq!(result, Variant::Bool(true));
}

#[test]
fn gdscript_point_outside_triangle() {
    let result = run_val(
        "return Geometry2D.point_is_inside_triangle(\
           Vector2(20, 20), Vector2(0, 0), Vector2(10, 0), Vector2(5, 10))\n",
    );
    assert_eq!(result, Variant::Bool(false));
}

#[test]
fn gdscript_point_on_triangle_edge() {
    // Point on edge (0,0)-(10,0) at (5, 0).
    let result = run_val(
        "return Geometry2D.point_is_inside_triangle(\
           Vector2(5, 0), Vector2(0, 0), Vector2(10, 0), Vector2(5, 10))\n",
    );
    // On-edge should be considered inside (matching Godot behavior).
    assert_eq!(result, Variant::Bool(true));
}

#[test]
fn gdscript_point_at_triangle_vertex() {
    let result = run_val(
        "return Geometry2D.point_is_inside_triangle(\
           Vector2(0, 0), Vector2(0, 0), Vector2(10, 0), Vector2(5, 10))\n",
    );
    assert_eq!(result, Variant::Bool(true));
}

#[test]
fn gdscript_point_is_inside_triangle_wrong_arity() {
    let mut interp = Interpreter::new();
    let result = interp.run(
        "return Geometry2D.point_is_inside_triangle(Vector2(0, 0), Vector2(1, 0))\n",
    );
    assert!(result.is_err(), "point_is_inside_triangle with 2 args should error");
}

// ===========================================================================
// 4. is_polygon_clockwise — GDScript reachability
// ===========================================================================

#[test]
fn gdscript_polygon_clockwise_true() {
    // CW polygon: (0,0) → (0,10) → (10,10) → (10,0)
    let result = run_val(
        "var poly = [Vector2(0, 0), Vector2(0, 10), Vector2(10, 10), Vector2(10, 0)]\n\
         return Geometry2D.is_polygon_clockwise(poly)\n",
    );
    assert_eq!(result, Variant::Bool(true));
}

#[test]
fn gdscript_polygon_counterclockwise_false() {
    // CCW polygon: (0,0) → (10,0) → (10,10) → (0,10)
    let result = run_val(
        "var poly = [Vector2(0, 0), Vector2(10, 0), Vector2(10, 10), Vector2(0, 10)]\n\
         return Geometry2D.is_polygon_clockwise(poly)\n",
    );
    assert_eq!(result, Variant::Bool(false));
}

#[test]
fn gdscript_polygon_clockwise_triangle() {
    // CW triangle
    let result = run_val(
        "var poly = [Vector2(0, 0), Vector2(0, 10), Vector2(10, 0)]\n\
         return Geometry2D.is_polygon_clockwise(poly)\n",
    );
    assert_eq!(result, Variant::Bool(true));
}

#[test]
fn gdscript_polygon_clockwise_degenerate() {
    // Less than 3 vertices → false
    let result = run_val(
        "var poly = [Vector2(0, 0), Vector2(10, 0)]\n\
         return Geometry2D.is_polygon_clockwise(poly)\n",
    );
    assert_eq!(result, Variant::Bool(false));
}

#[test]
fn gdscript_is_polygon_clockwise_wrong_arity() {
    let mut interp = Interpreter::new();
    let result = interp.run("return Geometry2D.is_polygon_clockwise()\n");
    assert!(result.is_err(), "is_polygon_clockwise with 0 args should error");
}

// ===========================================================================
// 5. Rust-level unit tests for new functions
// ===========================================================================

#[test]
fn rust_line_intersects_line_basic() {
    use gdcore::geometry2d::line_intersects_line;
    use gdcore::math::Vector2;

    let hit = line_intersects_line(
        Vector2::new(0.0, 0.0),
        Vector2::new(1.0, 0.0),
        Vector2::new(5.0, -10.0),
        Vector2::new(0.0, 1.0),
    );
    let h = hit.unwrap();
    assert_near(h.x, 5.0, 1e-4, "rust line hit.x");
    assert_near(h.y, 0.0, 1e-4, "rust line hit.y");
}

#[test]
fn rust_line_intersects_line_parallel() {
    use gdcore::geometry2d::line_intersects_line;
    use gdcore::math::Vector2;

    let hit = line_intersects_line(
        Vector2::new(0.0, 0.0),
        Vector2::new(1.0, 0.0),
        Vector2::new(0.0, 5.0),
        Vector2::new(1.0, 0.0),
    );
    assert!(hit.is_none(), "parallel lines should return None");
}

#[test]
fn rust_point_is_inside_triangle_inside() {
    use gdcore::geometry2d::point_is_inside_triangle;
    use gdcore::math::Vector2;

    assert!(point_is_inside_triangle(
        Vector2::new(3.0, 3.0),
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 0.0),
        Vector2::new(5.0, 10.0),
    ));
}

#[test]
fn rust_point_is_inside_triangle_outside() {
    use gdcore::geometry2d::point_is_inside_triangle;
    use gdcore::math::Vector2;

    assert!(!point_is_inside_triangle(
        Vector2::new(20.0, 20.0),
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 0.0),
        Vector2::new(5.0, 10.0),
    ));
}

#[test]
fn rust_is_polygon_clockwise() {
    use gdcore::geometry2d::is_polygon_clockwise;
    use gdcore::math::Vector2;

    // CW
    let cw = [
        Vector2::new(0.0, 0.0),
        Vector2::new(0.0, 10.0),
        Vector2::new(10.0, 10.0),
        Vector2::new(10.0, 0.0),
    ];
    assert!(is_polygon_clockwise(&cw));

    // CCW
    let ccw = [
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 0.0),
        Vector2::new(10.0, 10.0),
        Vector2::new(0.0, 10.0),
    ];
    assert!(!is_polygon_clockwise(&ccw));
}

#[test]
fn rust_closest_unclamped_extends() {
    use gdcore::geometry2d::get_closest_point_to_segment_unclamped;
    use gdcore::math::Vector2;

    let p = get_closest_point_to_segment_unclamped(
        Vector2::new(-10.0, 5.0),
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 0.0),
    );
    assert_near(p.x, -10.0, 1e-4, "unclamped x");
    assert_near(p.y, 0.0, 1e-4, "unclamped y");
}

// ===========================================================================
// 6. Stub boundary — methods that exist in Godot but are not yet implemented
// ===========================================================================
// These tests document the boundary: calling these methods returns a clear
// UndefinedFunction error. As methods are implemented, move them to the
// active bindings section above.

#[test]
fn stub_clip_polygons_errors() {
    let mut interp = Interpreter::new();
    let result = interp.run("return Geometry2D.clip_polygons([], [])\n");
    assert!(result.is_err(), "clip_polygons should error (not yet implemented)");
}

#[test]
fn stub_intersect_polygons_errors() {
    let mut interp = Interpreter::new();
    let result = interp.run("return Geometry2D.intersect_polygons([], [])\n");
    assert!(result.is_err(), "intersect_polygons should error (not yet implemented)");
}

#[test]
fn stub_merge_polygons_errors() {
    let mut interp = Interpreter::new();
    let result = interp.run("return Geometry2D.merge_polygons([], [])\n");
    assert!(result.is_err(), "merge_polygons should error (not yet implemented)");
}

#[test]
fn stub_exclude_polygons_errors() {
    let mut interp = Interpreter::new();
    let result = interp.run("return Geometry2D.exclude_polygons([], [])\n");
    assert!(result.is_err(), "exclude_polygons should error (not yet implemented)");
}

#[test]
fn stub_offset_polygon_errors() {
    let mut interp = Interpreter::new();
    let result = interp.run("return Geometry2D.offset_polygon([], 1.0)\n");
    assert!(result.is_err(), "offset_polygon should error (not yet implemented)");
}

#[test]
fn stub_offset_polyline_errors() {
    let mut interp = Interpreter::new();
    let result = interp.run("return Geometry2D.offset_polyline([], 1.0)\n");
    assert!(result.is_err(), "offset_polyline should error (not yet implemented)");
}

#[test]
fn stub_make_atlas_errors() {
    let mut interp = Interpreter::new();
    let result = interp.run("return Geometry2D.make_atlas([])\n");
    assert!(result.is_err(), "make_atlas should error (not yet implemented)");
}

#[test]
fn stub_decompose_polygon_in_convex_errors() {
    let mut interp = Interpreter::new();
    let result = interp.run("return Geometry2D.decompose_polygon_in_convex([])\n");
    assert!(result.is_err(), "decompose_polygon_in_convex should error (not yet implemented)");
}

// ===========================================================================
// 7. Full API surface inventory — all 8 bound methods accessible
// ===========================================================================

#[test]
fn all_bound_methods_accessible() {
    // Verify that all 8 currently bound methods are callable without error.
    let methods_and_calls = [
        ("build_arc", "Geometry2D.build_arc(Vector2(0,0), 1.0, 0.0, 3.14, 5)"),
        ("is_point_in_polygon", "Geometry2D.is_point_in_polygon(Vector2(5,5), [Vector2(0,0), Vector2(10,0), Vector2(10,10), Vector2(0,10)])"),
        ("segment_intersects_segment", "Geometry2D.segment_intersects_segment(Vector2(0,0), Vector2(10,10), Vector2(10,0), Vector2(0,10))"),
        ("get_closest_point_to_segment", "Geometry2D.get_closest_point_to_segment(Vector2(5,10), Vector2(0,0), Vector2(10,0))"),
        ("get_closest_point_to_segment_unclamped", "Geometry2D.get_closest_point_to_segment_unclamped(Vector2(5,10), Vector2(0,0), Vector2(10,0))"),
        ("line_intersects_line", "Geometry2D.line_intersects_line(Vector2(0,0), Vector2(1,0), Vector2(5,-10), Vector2(0,1))"),
        ("point_is_inside_triangle", "Geometry2D.point_is_inside_triangle(Vector2(5,5), Vector2(0,0), Vector2(10,0), Vector2(5,10))"),
        ("is_polygon_clockwise", "Geometry2D.is_polygon_clockwise([Vector2(0,0), Vector2(10,0), Vector2(10,10)])"),
    ];

    for (name, call) in &methods_and_calls {
        let src = format!("return {call}\n");
        let mut interp = Interpreter::new();
        let result = interp.run(&src);
        assert!(
            result.is_ok(),
            "Geometry2D.{name} should be callable: {:?}",
            result.err()
        );
    }
}
