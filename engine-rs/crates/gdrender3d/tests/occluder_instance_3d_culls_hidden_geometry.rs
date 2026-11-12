//! pat-nzgdu.212 — the occluder resources are registered and an occlusion-cull
//! pass skips fully-occluded instances; **removing or hiding the occluder
//! un-culls them**.
//!
//! ⚠️ **This bead is the third on the same subject.** The audit sweep filed it
//! three times under three keys: `.207` (`occluder-family`) registered the seven
//! ClassDB entries, `.211` (`occluder` + `sweep-7/occluder`) wired the cull pass
//! into both draw loops and fixed the occlusion math, and `.212`
//! (`occluder-resources`) asks for both again. Its premise — *"no renderer uses
//! it"* — was true when the sweep ran and is stale now.
//!
//! The one clause the earlier two did not cover is **"hiding the occluder
//! un-culls it"**, which was not expressible: the backend had no `visible`. That
//! is real parity, not a convenience — `OccluderInstance3D` is a
//! `VisualInstance3D`, and hiding one reaches `instance_set_visible`, which for
//! `INSTANCE_OCCLUDER` calls `scenario_set_instance(..., p_visible)` and drops
//! it from the occlusion scenario (`renderer_scene_cull.cpp:1075-1079`). So a
//! hidden occluder occludes nothing.
//!
//! `visible` is `Node3D`'s property, not one of `OccluderInstance3D`'s three own
//! members (`occluder`, `bake_mask`, `bake_simplification_distance`), so ClassDB
//! inherits it rather than re-declaring it.
//!
//! The bead's *registration* clause (`class_exists` for
//! `QuadOccluder3D`/`PolygonOccluder3D` under `Occluder3D:Resource`, and
//! `OccluderInstance3D`) is asserted by `gdobject`'s
//! `box_occluder3d_registered_and_culls` (pat-nzgdu.207), which is where it can
//! be: **gdrender3d does not depend on gdobject**, so ClassDB is not reachable
//! from this crate's tests. This file covers the half that lives here — the cull
//! pass.

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Basis, Transform3D};
use gdrender3d::renderer::SoftwareRenderer3D;
use gdserver3d::material::Material3D;
use gdserver3d::mesh::Mesh3D;
use gdserver3d::occluder::{Occluder3D, OccluderInstance3D};
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;

/// A unit cube at `(0, 0, z)`. The default camera sits at the origin looking
/// down -Z, so a negative `z` is in front of it.
fn cube_at(renderer: &mut SoftwareRenderer3D, z: f32) {
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    renderer.set_material(id, Material3D::default());
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, z),
        },
    );
}

fn nonblack(renderer: &mut SoftwareRenderer3D) -> usize {
    let vp = Viewport3D::new(64, 64);
    let frame = renderer.render_frame(&vp);
    frame.pixels.iter().filter(|c| **c != Color::BLACK).count()
}

/// The acceptance, end to end: culled → hidden un-culls → shown re-culls →
/// removed un-culls.
#[test]
fn occluder_instance_3d_culls_hidden_geometry() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(&mut renderer, -5.0);

    // Baseline against the scene itself rather than a pixel threshold — the
    // count depends on the camera geometry, the claims below do not.
    let visible_px = nonblack(&mut renderer);
    assert!(visible_px > 0, "baseline: the cube draws with no occluder");

    // (1) An occluder between camera and cube culls it.
    let mut occ =
        OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(10.0, 10.0, 2.0)));
    occ.transform = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(0.0, 0.0, -2.0),
    };
    renderer.add_occluder(occ);
    assert_eq!(
        nonblack(&mut renderer),
        0,
        "the cube is behind the occluder and must be culled"
    );

    // (2) ⚠️ HIDING the occluder un-culls it — the clause .207/.211 could not
    // express, because the backend had no `visible`.
    renderer.occluders_mut()[0].visible = false;
    assert_eq!(
        nonblack(&mut renderer),
        visible_px,
        "hiding the occluder must restore the cube exactly as it was"
    );

    // (3) Showing it again culls once more: visibility is a gate, not a
    // one-way teardown.
    renderer.occluders_mut()[0].visible = true;
    assert_eq!(
        nonblack(&mut renderer),
        0,
        "showing the occluder must cull again"
    );

    // (4) REMOVING it un-culls too — and proves the cull never mutated the
    // instance itself.
    renderer.clear_occluders();
    assert_eq!(
        nonblack(&mut renderer),
        visible_px,
        "removing the occluder must restore the cube"
    );
}

/// A hidden occluder is inert from the moment it is added — not merely after
/// being toggled. The default-constructed state is `visible: true`, so this
/// pins the gate itself rather than the toggle path.
#[test]
fn an_occluder_added_already_hidden_never_culls() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(&mut renderer, -5.0);
    let visible_px = nonblack(&mut renderer);
    assert!(visible_px > 0, "baseline: the cube draws");

    let mut occ =
        OccluderInstance3D::with_occluder(Occluder3D::new_box(Vector3::new(10.0, 10.0, 2.0)));
    occ.transform = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(0.0, 0.0, -2.0),
    };
    occ.visible = false;
    renderer.add_occluder(occ);

    assert_eq!(
        nonblack(&mut renderer),
        visible_px,
        "an occluder added while hidden must never cull"
    );
    // Not vacuous: the same occluder culls once shown.
    renderer.occluders_mut()[0].visible = true;
    assert_eq!(nonblack(&mut renderer), 0);
}
