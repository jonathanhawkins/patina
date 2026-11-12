//! pat-s3rrv.241 — `PathFollow2D` honours `cubic_interp`.
//!
//! Godot ground truth: `upstream/godot/scene/2d/path_2d.cpp`,
//! `PathFollow2D::_update_transform`, which samples
//! `curve->sample_baked(progress, cubic_interp)`.
//!
//! # What was wrong
//!
//! The **3D** follower has honoured `cubic_interp` since pat-s3rrv.91
//! (`Curve3D::sample_baked_cubic`, and the branch in `main_loop.rs` that reads
//! the property). Its 2D twin sampled linearly no matter what the property
//! said — `Curve2D` had no cubic sampler at all, and `Vector2` had no
//! `cubic_interpolate` for one to call, even though Godot's Vector2 has always
//! had it.
//!
//! # ⚠️ The fixture must be genuinely curved
//!
//! Catmull-Rom through **collinear** points reproduces the straight line
//! exactly. On a straight path — which is what most PathFollow fixtures use —
//! cubic and linear agree to the bit, so a test built on one would pass while
//! reading nothing at all. Every fixture here bends.
//!
//! # ⚠️ And the default is TRUE
//!
//! An **absent** `cubic_interp` must sample CUBIC. Reading it as
//! false-by-default is the natural Rust reflex (`matches!(.., Bool(true))`) and
//! would silently downgrade every scene that never set the property — with no
//! error, and invisible on any straight path.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A deliberately bent path: three segments that turn a corner.
fn curve() -> Vec<Vector2> {
    vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(1.0, 0.0),
        Vector2::new(2.0, 1.0),
        Vector2::new(3.0, 3.0),
    ]
}

/// Steps one frame and reports where the follower landed.
fn sampled(cubic: Option<bool>, progress: f32) -> Vector2 {
    sampled_with(cubic, progress, true)
}

/// As [`sampled`], but with control over `loop`.
///
/// ⚠️ `loop` DEFAULTS TO TRUE, and `PathFollow2D::advance` then applies
/// `progress.rem_euclid(length)`. So a progress past the end **wraps around**
/// rather than clamping — with this fixture, `progress = 1000` lands at
/// `1000 mod 4.650282 = 0.189`, a quarter of the way into the first segment.
/// Any test that means "sample the far end" has to turn looping off first, or
/// it is silently asserting about a mid-curve point.
fn sampled_with(cubic: Option<bool>, progress: f32, looping: bool) -> Vector2 {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut path = Node::new("Path", "Path2D");
    path.set_property("curve_points", Variant::PackedVector2Array(curve()));
    let path_id = tree.add_child(root, path).unwrap();

    let mut follow = Node::new("Follow", "PathFollow2D");
    follow.set_property("progress", Variant::Float(progress as f64));
    follow.set_property("loop", Variant::Bool(looping));
    if let Some(c) = cubic {
        follow.set_property("cubic_interp", Variant::Bool(c));
    }
    let fid = tree.add_child(path_id, follow).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    gdscene::node2d::get_position(ml.tree(), fid)
}

/// 🔑 **THE ACCEPTANCE CLAUSE.** Cubic and linear disagree mid-segment, and the
/// cubic result sits **off the straight chord**.
#[test]
fn a_cubic_interp_changes_the_sampled_position() {
    let progress = 1.5;
    let linear = sampled(Some(false), progress);
    let cubic = sampled(Some(true), progress);

    assert!(
        (cubic - linear).length() > 1e-4,
        "cubic and linear must differ mid-segment. Equal means cubic_interp is \
         being ignored — or the fixture went straight, which would make the two \
         agree legitimately. linear={linear:?} cubic={cubic:?}"
    );

    // 🔑 "Different" is not enough: it must be different in the RIGHT
    // direction. The linear sample lies exactly on the chord between the two
    // baked points it interpolates; a real Catmull-Rom sample bows off it.
    // Without this, any arbitrary perturbation would satisfy the assertion.
    let (a, b) = (curve()[1], curve()[2]);
    let chord = b - a;
    let to_cubic = cubic - a;
    let cross = chord.x * to_cubic.y - chord.y * to_cubic.x;
    assert!(
        cross.abs() > 1e-4,
        "the cubic sample must lie OFF the chord {a:?}->{b:?}, got {cubic:?} \
         (cross={cross})"
    );
}

/// 🔑 An absent `cubic_interp` means CUBIC, matching Godot's default.
///
/// ⚠️ This is the likelier bug of the two, and the cheaper one to get wrong.
#[test]
fn b_the_default_is_cubic_not_linear() {
    let progress = 1.5;
    let default = sampled(None, progress);
    let cubic = sampled(Some(true), progress);
    let linear = sampled(Some(false), progress);

    assert!(
        (default - cubic).length() < 1e-5,
        "an absent cubic_interp must sample CUBIC (Godot's default is true). \
         default={default:?} cubic={cubic:?}"
    );
    assert!(
        (default - linear).length() > 1e-4,
        "…and therefore must NOT match the linear sample. Matching means the \
         property is being read as false-by-default"
    );
}

/// The endpoints are shared by both modes.
///
/// ⚠️ Catmull-Rom clamps its neighbours at the ends rather than wrapping, so
/// the curve must still pass exactly through its first and last control points.
/// A wrapping implementation bows the end segments outward and fails here while
/// still looking smooth.
#[test]
fn c_both_modes_agree_at_the_endpoints() {
    // ⚠️ Looping OFF. With Godot's default `loop = true`, `progress = 1000`
    // wraps to `1000 mod 4.650282 = 0.189` — a mid-curve point where the two
    // modes legitimately DISAGREE. The first version of this test asserted
    // endpoint agreement while sampling nowhere near an endpoint.
    for (label, progress) in [("start", 0.0f32), ("end", 1000.0f32)] {
        let linear = sampled_with(Some(false), progress, false);
        let cubic = sampled_with(Some(true), progress, false);
        assert!(
            (cubic - linear).length() < 1e-5,
            "{label}: the two modes must agree where the curve is pinned. \
             linear={linear:?} cubic={cubic:?}"
        );
    }

    // And the endpoint really is the control point, not something near it —
    // an offset past the end clamps rather than extrapolating.
    let end = sampled_with(Some(true), 1000.0, false);
    let last = *curve().last().unwrap();
    assert!(
        (end - last).length() < 1e-5,
        "sampling past the end must clamp to the last point {last:?}, got {end:?}"
    );
}

/// 🔑 `Vector2::cubic_interpolate`'s argument order, pinned at the endpoints.
///
/// ⚠️ The method takes `(b, pre_a, post_b, weight)` while the scalar helper it
/// delegates to takes `(from, to, pre, post, w)` — so `b` is the SECOND scalar
/// argument, not the third. A transposition still yields a smooth curve through
/// plausible values, and only the endpoints catch it: `w == 0` must be exactly
/// `self` and `w == 1` exactly `b`.
#[test]
fn d_vector2_cubic_interpolate_endpoints_are_exact() {
    let a = Vector2::new(1.0, 2.0);
    let b = Vector2::new(5.0, -3.0);
    let pre = Vector2::new(-4.0, 9.0);
    let post = Vector2::new(11.0, 7.0);

    let at0 = a.cubic_interpolate(b, pre, post, 0.0);
    assert!(
        (at0 - a).length() < 1e-6,
        "weight 0 must be exactly `self`, got {at0:?}"
    );
    let at1 = a.cubic_interpolate(b, pre, post, 1.0);
    assert!(
        (at1 - b).length() < 1e-6,
        "weight 1 must be exactly `b`, got {at1:?}"
    );

    // Both components are interpolated, not just x — a one-component copy/paste
    // leaves y equal to `self.y` for every weight.
    let mid = a.cubic_interpolate(b, pre, post, 0.5);
    assert!(
        (mid.y - a.y).abs() > 1e-6,
        "y must interpolate too, got {mid:?}"
    );
}
