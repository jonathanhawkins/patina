//! pat-s3rrv.199: one primitive-mesh resolver, owning Godot's defaults.
//!
//! The bead's headline symptom is that `fixtures/scenes/demo_3d.tscn`, which
//! carries a `[sub_resource type="TorusMesh"]`, drew a torus in the editor and
//! **nothing** in the runner. The cause was two resolvers: the editor had a
//! `"TorusMesh"` arm with hardcoded stand-in numbers, the runtime resolver had
//! five types and no torus at all.
//!
//! `build_primitive` is the single resolver both now call, so the defaults exist
//! in exactly one place and cannot drift apart again. These tests pin that.

use gdserver3d::primitive_mesh::{build_primitive, TorusMesh, TubeTrailMesh};

/// No lookup at all — every property falls back to the built-in default.
fn defaults(_key: &str) -> Option<f32> {
    None
}

fn approx(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

/// Distance from the Y axis, which is what the torus's radii describe.
fn radial(v: &gdcore::math::Vector3) -> f32 {
    (v.x * v.x + v.z * v.z).sqrt()
}

/// Godot's documented TorusMesh defaults, transcribed from
/// `doc/classes/TorusMesh.xml`: 0.5 / 1.0 / rings 64 / ring_segments 32.
#[test]
fn an_unauthored_torus_uses_godots_documented_defaults() {
    let d = TorusMesh::default();
    assert_eq!(
        (d.inner_radius, d.outer_radius, d.rings, d.ring_segments),
        (0.5, 1.0, 64, 32)
    );

    let mesh = build_primitive("TorusMesh", &defaults).expect("TorusMesh is generated");

    // The seam ring is duplicated so UVs can wrap, hence +1 on each axis.
    assert_eq!(
        mesh.vertices.len(),
        (64 + 1) * (32 + 1),
        "a torus is (rings+1) x (ring_segments+1) vertices"
    );

    let max = mesh.vertices.iter().map(radial).fold(f32::MIN, f32::max);
    let min = mesh.vertices.iter().map(radial).fold(f32::MAX, f32::min);
    assert!(
        approx(max, 1.0, 1e-4),
        "the far edge of the tube is outer_radius, got {max}"
    );
    assert!(
        approx(min, 0.5, 1e-4),
        "the hole is inner_radius, got {min}"
    );
}

/// The whole point of the shared resolver: authored values must win.
///
/// ⚠️ Before this, the runtime resolver called `::default()` unconditionally, so
/// an authored radius was read from the scene, stored, and then ignored.
#[test]
fn authored_properties_beat_the_defaults() {
    let authored = |key: &str| match key {
        "inner_radius" => Some(0.25),
        "outer_radius" => Some(2.0),
        "rings" => Some(8.0),
        "ring_segments" => Some(6.0),
        _ => None,
    };
    let mesh = build_primitive("TorusMesh", &authored).expect("TorusMesh is generated");

    assert_eq!(mesh.vertices.len(), (8 + 1) * (6 + 1));

    let max = mesh.vertices.iter().map(radial).fold(f32::MIN, f32::max);
    let min = mesh.vertices.iter().map(radial).fold(f32::MAX, f32::min);
    assert!(approx(max, 2.0, 1e-4), "authored outer_radius, got {max}");
    assert!(approx(min, 0.25, 1e-4), "authored inner_radius, got {min}");

    // And it is genuinely not the default mesh.
    let default_mesh = build_primitive("TorusMesh", &defaults).unwrap();
    assert_ne!(mesh.vertices.len(), default_mesh.vertices.len());
}

/// `rings` and `ring_segments` must not be swapped.
///
/// ⚠️ Vertex count CANNOT catch this: the mesh is
/// `(rings+1) * (ring_segments+1)`, which is symmetric in the two, so a
/// transposed pair yields an identical count. They have to be told apart
/// geometrically — `ring_segments` walks the tube cross-section and is
/// therefore what controls the spread in Y, while `rings` walks the major
/// circle and controls the silhouette in XZ.
#[test]
fn rings_and_ring_segments_are_not_transposed() {
    let authored = |key: &str| match key {
        // Many steps around the ring, very few around the tube.
        "rings" => Some(32.0),
        "ring_segments" => Some(4.0),
        _ => None,
    };
    let mesh = build_primitive("TorusMesh", &authored).expect("TorusMesh is generated");

    // With 4 tube segments the cross-section only visits y = 0, +r, 0, -r.
    let mut ys: Vec<i64> = mesh
        .vertices
        .iter()
        .map(|v| (v.y * 10_000.0).round() as i64)
        .collect();
    ys.sort_unstable();
    ys.dedup();
    assert_eq!(
        ys.len(),
        3,
        "a 4-segment tube gives exactly three distinct Y values (0, +r, -r); \
         got {} — rings and ring_segments are swapped",
        ys.len()
    );

    // The XZ silhouette, driven by `rings`, is the smooth one here.
    let mut radii: Vec<i64> = mesh
        .vertices
        .iter()
        .map(|v| (radial(v) * 10_000.0).round() as i64)
        .collect();
    radii.sort_unstable();
    radii.dedup();
    assert!(
        radii.len() >= 3,
        "the major circle should produce several distinct radii"
    );
}

/// The specific numbers the editor used to carry, as a regression guard.
///
/// The editor's arm read `torus(0.22, 0.45, 24, 12)` — a stand-in, not Godot's
/// values. If either caller ever reintroduces private fallbacks, the shared
/// resolver stops being the single source and this catches the reappearance.
#[test]
fn the_editors_old_standin_torus_is_gone() {
    let mesh = build_primitive("TorusMesh", &defaults).expect("TorusMesh is generated");
    assert_ne!(
        mesh.vertices.len(),
        (24 + 1) * (12 + 1),
        "the old editor stand-in geometry must not be what the resolver returns"
    );

    let max = mesh.vertices.iter().map(radial).fold(f32::MIN, f32::max);
    assert!(
        !approx(max, 0.45, 1e-3),
        "outer radius must be Godot's 1.0, not the editor's 0.45"
    );
}

/// Every type the two callers previously handled must still resolve.
#[test]
fn all_previously_supported_types_still_resolve() {
    for class in [
        "BoxMesh",
        "CubeMesh",
        "SphereMesh",
        "CapsuleMesh",
        "CylinderMesh",
        "PlaneMesh",
        "QuadMesh",
        "PrismMesh",
        "TorusMesh",
        "PointMesh",
    ] {
        let mesh = build_primitive(class, &defaults)
            .unwrap_or_else(|| panic!("{class} must resolve through the shared resolver"));
        assert!(
            !mesh.vertices.is_empty(),
            "{class} must generate geometry, not an empty mesh"
        );
    }
}

/// An unknown class is `None`, so a caller can pick its own placeholder rather
/// than be handed the wrong shape.
#[test]
fn an_unknown_class_resolves_to_nothing() {
    assert!(build_primitive("NotAMesh", &defaults).is_none());
    assert!(build_primitive("", &defaults).is_none());
}

/// Malformed counts must not wrap into an enormous allocation.
///
/// A negative float cast straight to `u32` in Rust saturates at 0, which would
/// make a degenerate mesh; worse, a caller encoding "unset" as -1 would get a
/// silent empty torus rather than the default one.
#[test]
fn a_nonsensical_segment_count_is_clamped_not_wrapped() {
    let authored = |key: &str| match key {
        "rings" => Some(-8.0),
        "ring_segments" => Some(0.0),
        _ => None,
    };
    let mesh = build_primitive("TorusMesh", &authored).expect("TorusMesh is generated");

    assert_eq!(
        mesh.vertices.len(),
        (1 + 1) * (1 + 1),
        "both counts clamp to a minimum of one segment"
    );
    for v in &mesh.vertices {
        assert!(
            v.x.is_finite() && v.y.is_finite() && v.z.is_finite(),
            "degenerate input must still produce finite geometry"
        );
    }
}

// ---------------------------------------------------------------------------
// TubeTrailMesh, ported from primitive_meshes.cpp rather than reconstructed.
// ---------------------------------------------------------------------------

/// Godot's documented TubeTrailMesh defaults.
#[test]
fn tube_trail_defaults_match_the_class_docs() {
    let d = TubeTrailMesh::default();
    assert_eq!(d.radius, 0.5);
    assert_eq!(d.radial_steps, 8);
    assert_eq!(d.sections, 5);
    assert_eq!(d.section_length, 0.2);
    assert_eq!(d.section_rings, 3);
    assert!(d.cap_top && d.cap_bottom);
}

/// The vertex count checked against **Godot's own allocation formula**.
///
/// `TubeTrailMesh::_create_mesh_array` reserves
/// `(total_rings + 1) * (radial_steps + 1) + 4 + radial_steps * 2` points. That
/// is an independent statement of the same geometry — it comes from upstream,
/// not from my loop — so agreeing with it is real evidence the port is faithful
/// rather than merely self-consistent. The `+ 4 + radial_steps * 2` is exactly
/// the two caps: each is one centre plus `radial_steps + 1` ring vertices.
#[test]
fn the_tube_matches_godots_own_point_count() {
    let d = TubeTrailMesh::default();
    let mesh = build_primitive("TubeTrailMesh", &defaults).expect("TubeTrailMesh is generated");

    let total_rings = d.section_rings * d.sections;
    let godot_reserve = (total_rings + 1) * (d.radial_steps + 1) + 4 + d.radial_steps * 2;
    assert_eq!(mesh.vertices.len(), godot_reserve as usize);
    assert_eq!(
        mesh.vertices.len(),
        164,
        "defaults give 16*9 body + 10 + 10"
    );
    assert_eq!(mesh.normals.len(), mesh.vertices.len());
    assert_eq!(mesh.uvs.len(), mesh.vertices.len());
}

/// Length and radius come from `section_length * sections` and `radius`.
#[test]
fn the_tube_spans_its_authored_depth_and_radius() {
    let mesh = build_primitive("TubeTrailMesh", &defaults).expect("generated");

    let max_y = mesh.vertices.iter().map(|v| v.y).fold(f32::MIN, f32::max);
    let min_y = mesh.vertices.iter().map(|v| v.y).fold(f32::MAX, f32::min);
    // depth = 0.2 * 5 = 1.0, centred on the origin.
    assert!(approx(max_y, 0.5, 1e-5), "top at +depth/2, got {max_y}");
    assert!(approx(min_y, -0.5, 1e-5), "bottom at -depth/2, got {min_y}");

    let max_r = mesh.vertices.iter().map(radial).fold(f32::MIN, f32::max);
    assert!(approx(max_r, 0.5, 1e-5), "radius 0.5, got {max_r}");
}

/// ⚠️ The seam vertex is not computed from its angle.
///
/// Godot forces `(x, z) = (0, 1)` at `i == radial_steps` so the closing vertex
/// is bit-identical to the opening one while still carrying `u = 1` for the UV
/// wrap. Computing `sin(TAU)` instead leaves a hairline crack that only shows
/// at some radii — the kind of thing a from-the-docs reconstruction gets wrong.
#[test]
fn the_seam_vertex_exactly_repeats_the_first() {
    let d = TubeTrailMesh::default();
    let mesh = d.generate();

    let first = mesh.vertices[0];
    let seam = mesh.vertices[d.radial_steps as usize];
    assert_eq!(
        (first.x, first.y, first.z),
        (seam.x, seam.y, seam.z),
        "the ring must close exactly, not merely nearly"
    );
    // ...while the UVs differ, which is why the duplicate exists at all.
    assert_eq!(mesh.uvs[0][0], 0.0);
    assert_eq!(mesh.uvs[d.radial_steps as usize][0], 1.0);
}

/// Each cap is one centre vertex plus a full ring.
#[test]
fn dropping_a_cap_drops_exactly_its_vertices() {
    let d = TubeTrailMesh::default();
    let both = d.generate().vertices.len();

    let no_top = TubeTrailMesh {
        cap_top: false,
        ..d.clone()
    }
    .generate()
    .vertices
    .len();
    let neither = TubeTrailMesh {
        cap_top: false,
        cap_bottom: false,
        ..d.clone()
    }
    .generate()
    .vertices
    .len();

    let per_cap = (d.radial_steps + 2) as usize;
    assert_eq!(
        both - no_top,
        per_cap,
        "one cap is centre + (radial_steps+1)"
    );
    assert_eq!(both - neither, per_cap * 2);
    assert_eq!(
        neither,
        ((d.section_rings * d.sections + 1) * (d.radial_steps + 1)) as usize,
        "an uncapped tube is just the body grid"
    );
}

/// Authored properties reach the tube, including the bools.
///
/// `cap_bottom` is a bool travelling through a numeric lookup as 0/1, which is
/// how the scene marker encodes it — a channel the other primitives never
/// exercise.
#[test]
fn authored_tube_properties_including_bools_are_applied() {
    let authored = |key: &str| match key {
        "radius" => Some(2.0),
        "sections" => Some(2.0),
        "section_length" => Some(1.5),
        "radial_steps" => Some(4.0),
        "section_rings" => Some(1.0),
        "cap_bottom" => Some(0.0),
        _ => None,
    };
    let mesh = build_primitive("TubeTrailMesh", &authored).expect("generated");

    // body (2*1 + 1) * (4+1) = 15, plus one cap of 4+2 = 6.
    assert_eq!(mesh.vertices.len(), 15 + 6);

    let max_r = mesh.vertices.iter().map(radial).fold(f32::MIN, f32::max);
    assert!(approx(max_r, 2.0, 1e-5), "authored radius, got {max_r}");

    let max_y = mesh.vertices.iter().map(|v| v.y).fold(f32::MIN, f32::max);
    let min_y = mesh.vertices.iter().map(|v| v.y).fold(f32::MAX, f32::min);
    // depth = 1.5 * 2 = 3.0
    assert!(
        approx(max_y - min_y, 3.0, 1e-5),
        "authored depth, got {}",
        max_y - min_y
    );

    // With no bottom cap there is no centre vertex on the axis at the base.
    let base_centres = mesh
        .vertices
        .iter()
        .filter(|v| approx(v.y, min_y, 1e-5) && radial(v) < 1e-5)
        .count();
    assert_eq!(
        base_centres, 0,
        "cap_bottom = false must remove the fan centre"
    );
}

/// Degenerate counts must not produce an empty or non-finite tube.
#[test]
fn a_degenerate_tube_is_clamped() {
    let authored = |key: &str| match key {
        "radial_steps" => Some(0.0),
        "sections" => Some(0.0),
        "section_rings" => Some(0.0),
        _ => None,
    };
    let mesh = build_primitive("TubeTrailMesh", &authored).expect("generated");
    assert!(
        !mesh.vertices.is_empty(),
        "a clamped tube still has geometry"
    );
    for v in &mesh.vertices {
        assert!(v.x.is_finite() && v.y.is_finite() && v.z.is_finite());
    }
}

/// TubeTrailMesh now resolves; it was in the unknown-class list before.
#[test]
fn tube_trail_is_no_longer_an_unknown_class() {
    assert!(build_primitive("TubeTrailMesh", &defaults).is_some());
}
