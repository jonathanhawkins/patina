//! pat-s3rrv.59 (first slice): the core math types behind the six missing
//! Variant builtins.
//!
//! Godot 4.6.1 has 38 Variant builtins; six had **no Rust type anywhere in
//! Patina** — `Vector4`, `Vector4i`, `Rect2i`, `Projection`, `RID` and the
//! element type of `PackedVector4Array`. Until those exist in `gdcore`, the
//! `Variant` enum has nothing to hold, the `.tscn` loader has to smuggle the
//! literals through as `__gd_unparsed_type__` dictionaries, and `RID` — which
//! every `*Server` API returns — has nowhere to live at all.
//!
//! This pins the semantics that are easy to get subtly wrong, because each one
//! fails silently rather than loudly: half-open rect edges, integer-truncating
//! centres, and column-major matrix storage.

use gdcore::math::{Rect2i, Vector2i, Vector3, Vector4, Vector4i};
use gdcore::math3d::Projection;
use gdcore::Rid;

#[test]
fn vector4_arithmetic_and_length_match_godot() {
    let a = Vector4::new(1.0, 2.0, 3.0, 4.0);
    let b = Vector4::new(0.5, 0.5, 0.5, 0.5);

    assert_eq!(a + b, Vector4::new(1.5, 2.5, 3.5, 4.5));
    assert_eq!(a - b, Vector4::new(0.5, 1.5, 2.5, 3.5));
    assert_eq!(a * 2.0, Vector4::new(2.0, 4.0, 6.0, 8.0));
    assert_eq!(-a, Vector4::new(-1.0, -2.0, -3.0, -4.0));

    // dot and length agree with each other: |v|² == v·v.
    assert!((a.dot(a) - a.length_squared()).abs() < 1e-6);
    // (2,0,0,0) is length 2 — a case whose answer is exact in binary floats.
    assert_eq!(Vector4::new(2.0, 0.0, 0.0, 0.0).length(), 2.0);

    let n = Vector4::new(0.0, 3.0, 4.0, 0.0).normalized();
    assert!(n.is_normalized(), "normalized() must produce a unit vector");
    assert!((n.y - 0.6).abs() < 1e-6 && (n.z - 0.8).abs() < 1e-6);

    // ⚠️ Godot returns ZERO rather than NaN for a zero-length input, so a
    // default-constructed vector cannot poison whatever it feeds.
    assert_eq!(Vector4::ZERO.normalized(), Vector4::ZERO);

    assert_eq!(a.lerp(b, 0.0), a, "lerp at 0 is the start");
    assert_eq!(a.lerp(b, 1.0), b, "lerp at 1 is the end");
    assert_eq!(
        Vector4::new(-1.5, 2.5, -3.5, 4.5).abs(),
        Vector4::new(1.5, 2.5, 3.5, 4.5)
    );
}

#[test]
fn vector4i_stays_exact_and_saturates_its_length() {
    let a = Vector4i::new(1, -2, 3, -4);
    assert_eq!(a + Vector4i::ONE, Vector4i::new(2, -1, 4, -3));
    assert_eq!(-a, Vector4i::new(-1, 2, -3, 4));
    assert_eq!(a.abs(), Vector4i::new(1, 2, 3, 4));
    assert_eq!(a.sign(), Vector4i::new(1, -1, 1, -1));

    assert_eq!(
        a.min(Vector4i::ZERO),
        Vector4i::new(0, -2, 0, -4),
        "min is component-wise"
    );
    assert_eq!(a.max(Vector4i::ZERO), Vector4i::new(1, 0, 3, 0));
    assert_eq!(
        a.clamp(Vector4i::new(0, 0, 0, 0), Vector4i::new(2, 2, 2, 2)),
        Vector4i::new(1, 0, 2, 0)
    );

    // ⚠️ `length_squared` is i64 on purpose: MAX² overflows i32 by a mile, and
    // wrapping would report a huge vector as a tiny one.
    let big = Vector4i::new(i32::MAX, 0, 0, 0);
    assert_eq!(big.length_squared(), (i32::MAX as i64) * (i32::MAX as i64));

    assert_eq!(
        Vector4::from(Vector4i::new(1, 2, 3, 4)),
        Vector4::new(1.0, 2.0, 3.0, 4.0),
        "int → float widening is exact for small magnitudes"
    );
    assert_eq!(
        Vector4::new(1.9, -1.9, 0.4, -0.4).to_vector4i(),
        Vector4i::new(1, -1, 0, 0),
        "float → int truncates toward zero, as Godot's constructor does"
    );
}

#[test]
fn rect2i_edges_are_half_open_and_its_centre_truncates() {
    let r = Rect2i::from_components(0, 0, 16, 16);

    assert_eq!(r.end(), Vector2i::new(16, 16));
    assert_eq!(r.area(), 256);
    assert!(r.has_area());

    // ⚠️ Half-open: the low edge is inside, the high edge is not. Tiling atlas
    // regions depend on it — an inclusive high edge makes two neighbouring
    // tiles both claim the boundary pixel.
    assert!(r.has_point(Vector2i::new(0, 0)), "low edge is inside");
    assert!(!r.has_point(Vector2i::new(16, 0)), "high edge is outside");
    assert!(!r.has_point(Vector2i::new(-1, 0)));

    // ⚠️ Integer division: a 15-wide rect centres at 7, not 7.5.
    assert_eq!(
        Rect2i::from_components(0, 0, 15, 15).get_center(),
        Vector2i::new(7, 7)
    );

    let other = Rect2i::from_components(8, 8, 16, 16);
    assert!(r.intersects(other));
    assert_eq!(
        r.intersection(other),
        Rect2i::from_components(8, 8, 8, 8),
        "the overlap of two 16² rects offset by 8 is an 8² rect"
    );
    assert_eq!(r.merge(other), Rect2i::from_components(0, 0, 24, 24));

    // Touching rects do NOT intersect, for the same half-open reason.
    assert!(!r.intersects(Rect2i::from_components(16, 0, 4, 4)));

    assert!(r.encloses(Rect2i::from_components(2, 2, 4, 4)));
    assert!(!r.encloses(other));

    // A negative size names the same region from the other corner.
    assert_eq!(
        Rect2i::from_components(10, 10, -4, -4).abs(),
        Rect2i::from_components(6, 6, 4, 4)
    );
    assert_eq!(r.grow(2), Rect2i::from_components(-2, -2, 20, 20));
    assert_eq!(
        r.expand(Vector2i::new(20, 4)),
        Rect2i::from_components(0, 0, 20, 16),
        "expanding to a point outside widens only the axis that needed it"
    );
}

#[test]
fn projection_is_column_major_and_composes() {
    let p = Projection::IDENTITY;
    assert!(p.is_orthogonal(), "the identity has no perspective divide");
    let v = Vector4::new(1.0, 2.0, 3.0, 1.0);
    assert_eq!(p.xform(v), v, "the identity leaves a point alone");

    // ⚠️ Column-major storage: `from_columns` puts each Vector4 in ONE column,
    // so reading column 3 must give back the fourth argument. Getting the
    // convention backwards transposes every matrix silently, which is exactly
    // the bug this asserts against.
    let a = Vector4::new(1.0, 2.0, 3.0, 4.0);
    let b = Vector4::new(5.0, 6.0, 7.0, 8.0);
    let c = Vector4::new(9.0, 10.0, 11.0, 12.0);
    let d = Vector4::new(13.0, 14.0, 15.0, 16.0);
    let m = Projection::from_columns(a, b, c, d);
    assert_eq!(m.column(0), a);
    assert_eq!(m.column(3), d);
    assert_eq!(m.transposed().column(0), Vector4::new(1.0, 5.0, 9.0, 13.0));

    // The flat form is column-major too — the layout a GPU upload expects.
    assert_eq!(m.to_array()[0..4], [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(Projection::from_array(m.to_array()), m);

    // Identity is the multiplicative unit, which only holds if `mul` indexes
    // rows and columns the same way `xform` does.
    assert_eq!(m * Projection::IDENTITY, m);
    assert_eq!(Projection::IDENTITY * m, m);

    // A perspective projection is NOT orthogonal, and it puts -1 in the slot
    // that produces the perspective divide.
    let persp = Projection::create_perspective(90.0, 1.0, 0.1, 100.0);
    assert!(!persp.is_orthogonal());
    assert_eq!(persp.m[2][3], -1.0);
    // At a 90° vertical FOV with square aspect, x and y scale by 1.
    assert!((persp.m[0][0] - 1.0).abs() < 1e-5);
    assert!((persp.m[1][1] - 1.0).abs() < 1e-5);

    // An orthographic projection maps the near plane to -1 and the far to +1.
    let ortho = Projection::create_orthogonal(-1.0, 1.0, -1.0, 1.0, 0.0, 2.0);
    assert!(ortho.is_orthogonal());
    let near = ortho.xform(Vector4::new(0.0, 0.0, 0.0, 1.0));
    let far = ortho.xform(Vector4::new(0.0, 0.0, -2.0, 1.0));
    assert!(
        (near.z + 1.0).abs() < 1e-5,
        "near plane maps to -1, got {near:?}"
    );
    assert!(
        (far.z - 1.0).abs() < 1e-5,
        "far plane maps to +1, got {far:?}"
    );
}

#[test]
fn rid_is_an_opaque_handle_whose_zero_is_invalid() {
    assert!(!Rid::INVALID.is_valid());
    assert_eq!(Rid::INVALID.id(), 0);
    // A defaulted RID must be the invalid one — a server handle that defaulted
    // to a *valid*-looking id would alias whatever really owns that slot.
    assert_eq!(Rid::default(), Rid::INVALID);

    let handle = Rid::from_raw(42);
    assert!(handle.is_valid());
    assert_eq!(handle.id(), 42);
    assert_ne!(handle, Rid::INVALID);
    assert_eq!(handle, Rid::from_raw(42), "the same id is the same handle");
    assert_eq!(format!("{handle}"), "RID(42)");
}

/// The parts of `Projection` that make consolidating `gdscene::camera3d`'s
/// duplicate a move rather than a rewrite (pat-s3rrv.59), and whose failure
/// mode is a silently wrong picture rather than an error.
#[test]
fn projection_inverse_and_frustum_planes_are_self_consistent() {
    // `inverse` is solved by elimination rather than transcribed as a cofactor
    // table precisely so this property can check it: a matrix times its inverse
    // is the identity, whatever the matrix.
    for p in [
        Projection::create_perspective(60.0, 16.0 / 9.0, 0.1, 1000.0),
        Projection::create_orthogonal(-2.0, 2.0, -1.5, 1.5, 0.1, 50.0),
        Projection::create_orthogonal_aspect(3.0, 4.0 / 3.0, 0.5, 20.0),
    ] {
        let round_trip = p * p.inverse();
        for col in 0..4 {
            for row in 0..4 {
                let want = if col == row { 1.0 } else { 0.0 };
                assert!(
                    (round_trip.m[col][row] - want).abs() < 1e-3,
                    "p * p.inverse() must be the identity; [{col}][{row}] was {}",
                    round_trip.m[col][row]
                );
            }
        }
        assert!(
            p.determinant().abs() > 1e-9,
            "a real projection is invertible"
        );
    }

    // A singular matrix returns IDENTITY rather than infinities — a degenerate
    // camera should render wrongly, not poison everything downstream with NaN.
    assert_eq!(Projection::ZERO.inverse(), Projection::IDENTITY);
    assert_eq!(Projection::ZERO.determinant(), 0.0);

    // `create_orthogonal_aspect(size, aspect, …)` is the centred form of
    // `create_orthogonal`, which is the rename the consolidation needs.
    assert_eq!(
        Projection::create_orthogonal_aspect(2.0, 2.0, 0.0, 10.0),
        Projection::create_orthogonal(-2.0, 2.0, -1.0, 1.0, 0.0, 10.0)
    );

    // An axis-aligned orthographic box has axis-aligned frustum planes, and
    // every normal points INWARD — the sign convention that decides whether a
    // culling test keeps what it should or exactly the opposite.
    let ortho = Projection::create_orthogonal(-1.0, 1.0, -1.0, 1.0, 1.0, 3.0);
    let planes = ortho.get_frustum_planes();
    let inside = Vector3::new(0.0, 0.0, -2.0); // between the near and far planes
    for (i, plane) in planes.iter().enumerate() {
        assert!(
            (plane.normal.length() - 1.0).abs() < 1e-5,
            "frustum plane {i} must be normalized"
        );
        let signed =
            plane.normal.x * inside.x + plane.normal.y * inside.y + plane.normal.z * inside.z
                - plane.d;
        assert!(
            signed > 0.0,
            "a point inside the box must be on the positive side of plane {i} \
             (normals point inward), got {signed}"
        );
    }
    // left/right normals are ±X, bottom/top ±Y.
    assert!(
        (planes[0].normal.x - 1.0).abs() < 1e-5,
        "left plane faces +X"
    );
    assert!(
        (planes[1].normal.x + 1.0).abs() < 1e-5,
        "right plane faces -X"
    );
    assert!(
        (planes[2].normal.y - 1.0).abs() < 1e-5,
        "bottom plane faces +Y"
    );
    assert!(
        (planes[3].normal.y + 1.0).abs() < 1e-5,
        "top plane faces -Y"
    );
}
