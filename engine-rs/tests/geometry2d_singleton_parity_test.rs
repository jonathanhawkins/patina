//! pat-jwkv / pat-nc0w: Geometry2D singleton GDScript parity test.
//!
//! Verifies that `Geometry2D` arc helpers and other utility methods are
//! reachable through the GDScript interpreter as singleton method calls,
//! matching Godot's `Geometry2D.build_arc(...)` calling convention.
//!
//! This closes the gap where the underlying `gdcore::geometry2d` functions
//! existed but were not callable from GDScript parity fixtures.
//!
//! pat-nc0w extends coverage with 4.6.1-specific behavioral checks:
//! even angular spacing (fixed subdivision), radius invariant, offset
//! center, reverse sweep, full circle wrap, and arity error handling.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run a GDScript snippet and return the value from `return`.
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

// ===========================================================================
// 1. Geometry2D.build_arc — reachable from GDScript
// ===========================================================================

#[test]
fn singleton_build_arc_returns_array() {
    let result = run_val(
        "var pts = Geometry2D.build_arc(Vector2(0, 0), 1.0, 0.0, 3.14159, 10)\nreturn pts\n",
    );
    match &result {
        Variant::Array(arr) => assert_eq!(arr.len(), 10, "build_arc should return 10 points"),
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn singleton_build_arc_points_are_vector2() {
    let result = run_val(
        "return Geometry2D.build_arc(Vector2(0, 0), 5.0, 0.0, 1.5708, 4)\n",
    );
    if let Variant::Array(arr) = &result {
        for (i, v) in arr.iter().enumerate() {
            assert!(
                matches!(v, Variant::Vector2(_)),
                "point {i} should be Vector2, got {v:?}"
            );
        }
    } else {
        panic!("expected Array, got {result:?}");
    }
}

#[test]
fn singleton_build_arc_first_last_angles() {
    // Quarter-circle: start at 0, end at PI/2, radius 1, center origin.
    let result = run_val(
        "return Geometry2D.build_arc(Vector2(0, 0), 1.0, 0.0, 1.5707963, 10)\n",
    );
    if let Variant::Array(arr) = result {
        let first = match &arr[0] {
            Variant::Vector2(v) => *v,
            other => panic!("expected Vector2, got {other:?}"),
        };
        let last = match arr.last().unwrap() {
            Variant::Vector2(v) => *v,
            other => panic!("expected Vector2, got {other:?}"),
        };
        // First: angle=0 → (1, 0)
        assert_near(first.x, 1.0, 1e-4, "first.x");
        assert_near(first.y, 0.0, 1e-4, "first.y");
        // Last: angle=π/2 → (0, 1)
        assert_near(last.x, 0.0, 1e-4, "last.x");
        assert_near(last.y, 1.0, 1e-4, "last.y");
    } else {
        panic!("expected Array");
    }
}

#[test]
fn singleton_build_arc_degenerate_count() {
    let result = run_val("return Geometry2D.build_arc(Vector2(0, 0), 1.0, 0.0, 3.14, 1)\n");
    match &result {
        Variant::Array(arr) => assert!(arr.is_empty(), "point_count < 2 should return empty"),
        other => panic!("expected Array, got {other:?}"),
    }
}

// ===========================================================================
// 2. Geometry2D.is_point_in_polygon — reachable from GDScript
// ===========================================================================

#[test]
fn singleton_is_point_in_polygon_inside() {
    let result = run_val(
        "var poly = [Vector2(0, 0), Vector2(10, 0), Vector2(10, 10), Vector2(0, 10)]\n\
         return Geometry2D.is_point_in_polygon(Vector2(5, 5), poly)\n",
    );
    assert_eq!(result, Variant::Bool(true));
}

#[test]
fn singleton_is_point_in_polygon_outside() {
    let result = run_val(
        "var poly = [Vector2(0, 0), Vector2(10, 0), Vector2(10, 10), Vector2(0, 10)]\n\
         return Geometry2D.is_point_in_polygon(Vector2(15, 5), poly)\n",
    );
    assert_eq!(result, Variant::Bool(false));
}

// ===========================================================================
// 3. Geometry2D.segment_intersects_segment — reachable from GDScript
// ===========================================================================

#[test]
fn singleton_segment_intersects_hit() {
    // X cross at (5, 5)
    let result = run_val(
        "return Geometry2D.segment_intersects_segment(\
           Vector2(0, 0), Vector2(10, 10), \
           Vector2(10, 0), Vector2(0, 10))\n",
    );
    match result {
        Variant::Vector2(hit) => {
            assert_near(hit.x, 5.0, 1e-4, "hit.x");
            assert_near(hit.y, 5.0, 1e-4, "hit.y");
        }
        other => panic!("expected Vector2 intersection, got {other:?}"),
    }
}

#[test]
fn singleton_segment_intersects_miss() {
    // Parallel segments — no intersection
    let result = run_val(
        "return Geometry2D.segment_intersects_segment(\
           Vector2(0, 0), Vector2(10, 0), \
           Vector2(0, 5), Vector2(10, 5))\n",
    );
    assert_eq!(result, Variant::Nil, "parallel segments should return nil");
}

// ===========================================================================
// 4. Geometry2D.get_closest_point_to_segment — reachable from GDScript
// ===========================================================================

#[test]
fn singleton_get_closest_point_to_segment() {
    let result = run_val(
        "return Geometry2D.get_closest_point_to_segment(\
           Vector2(5, 10), Vector2(0, 0), Vector2(10, 0))\n",
    );
    match result {
        Variant::Vector2(p) => {
            assert_near(p.x, 5.0, 1e-4, "closest.x");
            assert_near(p.y, 0.0, 1e-4, "closest.y");
        }
        other => panic!("expected Vector2, got {other:?}"),
    }
}

#[test]
fn singleton_get_closest_point_clamped() {
    let result = run_val(
        "return Geometry2D.get_closest_point_to_segment(\
           Vector2(-10, 0), Vector2(0, 0), Vector2(10, 0))\n",
    );
    match result {
        Variant::Vector2(p) => {
            assert_near(p.x, 0.0, 1e-4, "clamped to start");
            assert_near(p.y, 0.0, 1e-4, "clamped y");
        }
        other => panic!("expected Vector2, got {other:?}"),
    }
}

// ===========================================================================
// 5. build_arc — 4.6.1 fixed-subdivision parity (pat-nc0w)
// ===========================================================================

#[test]
fn singleton_build_arc_even_angular_spacing() {
    // Core 4.6.1 behavioral change: fixed subdivision → equal angular gaps.
    // Quarter-circle with 5 points → 4 gaps of PI/8 each.
    // We return the points and compute angles in Rust since atan2 is not a
    // GDScript global builtin in Patina's interpreter.
    let result = run_val(
        "return Geometry2D.build_arc(Vector2(0, 0), 1.0, 0.0, 1.5707963, 5)\n",
    );
    if let Variant::Array(arr) = result {
        assert_eq!(arr.len(), 5);
        let expected_step: f32 = std::f32::consts::FRAC_PI_2 / 4.0;
        for i in 0..4 {
            let p0 = match &arr[i] {
                Variant::Vector2(v) => *v,
                other => panic!("point {i}: expected Vector2, got {other:?}"),
            };
            let p1 = match &arr[i + 1] {
                Variant::Vector2(v) => *v,
                other => panic!("point {}: expected Vector2, got {other:?}", i + 1),
            };
            let a0 = p0.y.atan2(p0.x);
            let a1 = p1.y.atan2(p1.x);
            assert_near(a1 - a0, expected_step, 1e-4, &format!("angular gap {i}"));
        }
    } else {
        panic!("expected Array");
    }
}

#[test]
fn singleton_build_arc_radius_invariant() {
    // Every point must lie at the requested radius from center.
    let result = run_val(
        "var center = Vector2(10, 20)\n\
         var radius = 50.0\n\
         var pts = Geometry2D.build_arc(center, radius, 0.0, 6.2831853, 32)\n\
         var dists = []\n\
         for p in pts:\n\
         \tvar dx = p.x - center.x\n\
         \tvar dy = p.y - center.y\n\
         \tdists.append(sqrt(dx * dx + dy * dy))\n\
         return dists\n",
    );
    if let Variant::Array(dists) = result {
        assert_eq!(dists.len(), 32);
        for (i, d) in dists.iter().enumerate() {
            let dist = match d {
                Variant::Float(f) => *f as f32,
                other => panic!("dist {i}: expected Float, got {other:?}"),
            };
            assert_near(dist, 50.0, 1e-2, &format!("point {i} radius"));
        }
    } else {
        panic!("expected Array of distances");
    }
}

#[test]
fn singleton_build_arc_offset_center() {
    // Center at (100, 200), radius 10, two points from 0 to PI/2.
    let result = run_val(
        "return Geometry2D.build_arc(Vector2(100, 200), 10.0, 0.0, 1.5707963, 2)\n",
    );
    if let Variant::Array(arr) = result {
        assert_eq!(arr.len(), 2);
        let p0 = match &arr[0] {
            Variant::Vector2(v) => *v,
            other => panic!("expected Vector2, got {other:?}"),
        };
        let p1 = match &arr[1] {
            Variant::Vector2(v) => *v,
            other => panic!("expected Vector2, got {other:?}"),
        };
        // First: center + (10, 0) = (110, 200)
        assert_near(p0.x, 110.0, 1e-3, "offset first.x");
        assert_near(p0.y, 200.0, 1e-3, "offset first.y");
        // Last: center + (0, 10) = (100, 210)
        assert_near(p1.x, 100.0, 1e-3, "offset last.x");
        assert_near(p1.y, 210.0, 1e-3, "offset last.y");
    } else {
        panic!("expected Array");
    }
}

#[test]
fn singleton_build_arc_reverse_sweep() {
    // Sweep from PI to 0 (clockwise). 3 points: PI, PI/2, 0.
    let result = run_val(
        "return Geometry2D.build_arc(Vector2(0, 0), 1.0, 3.14159265, 0.0, 3)\n",
    );
    if let Variant::Array(arr) = result {
        assert_eq!(arr.len(), 3);
        let first = match &arr[0] {
            Variant::Vector2(v) => *v,
            other => panic!("expected Vector2, got {other:?}"),
        };
        let last = match &arr[2] {
            Variant::Vector2(v) => *v,
            other => panic!("expected Vector2, got {other:?}"),
        };
        // First at PI → (-1, ~0)
        assert_near(first.x, -1.0, 1e-4, "reverse first.x");
        // Last at 0 → (1, ~0)
        assert_near(last.x, 1.0, 1e-4, "reverse last.x");
    } else {
        panic!("expected Array");
    }
}

#[test]
fn singleton_build_arc_full_circle_wraps() {
    // Full circle (0 to TAU): first and last points should coincide.
    let result = run_val(
        "return Geometry2D.build_arc(Vector2(0, 0), 5.0, 0.0, 6.2831853, 64)\n",
    );
    if let Variant::Array(arr) = result {
        assert_eq!(arr.len(), 64);
        let first = match &arr[0] {
            Variant::Vector2(v) => *v,
            other => panic!("expected Vector2, got {other:?}"),
        };
        let last = match arr.last().unwrap() {
            Variant::Vector2(v) => *v,
            other => panic!("expected Vector2, got {other:?}"),
        };
        assert_near(first.x, last.x, 1e-3, "full circle x wrap");
        assert_near(first.y, last.y, 1e-3, "full circle y wrap");
    } else {
        panic!("expected Array");
    }
}

#[test]
fn singleton_build_arc_count_zero_returns_empty() {
    let result = run_val(
        "return Geometry2D.build_arc(Vector2(0, 0), 1.0, 0.0, 3.14, 0)\n",
    );
    match &result {
        Variant::Array(arr) => assert!(arr.is_empty(), "point_count=0 should return empty"),
        other => panic!("expected Array, got {other:?}"),
    }
}

// ===========================================================================
// 6. Error handling — unknown method and wrong arity
// ===========================================================================

#[test]
fn singleton_unknown_method_errors() {
    let mut interp = Interpreter::new();
    let result = interp.run("return Geometry2D.nonexistent_method()\n");
    assert!(
        result.is_err(),
        "calling unknown Geometry2D method should error"
    );
}

#[test]
fn singleton_build_arc_wrong_arity_errors() {
    let mut interp = Interpreter::new();
    // Too few arguments
    let result = interp.run("return Geometry2D.build_arc(Vector2(0, 0), 1.0)\n");
    assert!(result.is_err(), "build_arc with 2 args should error");
}

#[test]
fn singleton_segment_intersects_wrong_arity_errors() {
    let mut interp = Interpreter::new();
    let result = interp.run(
        "return Geometry2D.segment_intersects_segment(Vector2(0, 0), Vector2(1, 1))\n",
    );
    assert!(
        result.is_err(),
        "segment_intersects_segment with 2 args should error"
    );
}

#[test]
fn singleton_is_point_in_polygon_wrong_arity_errors() {
    let mut interp = Interpreter::new();
    let result = interp.run("return Geometry2D.is_point_in_polygon(Vector2(0, 0))\n");
    assert!(
        result.is_err(),
        "is_point_in_polygon with 1 arg should error"
    );
}
