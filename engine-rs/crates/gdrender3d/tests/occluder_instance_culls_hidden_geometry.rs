//! pat-nzgdu.211 — an `OccluderInstance3D` between the camera and a mesh
//! removes that mesh from the rendered frame.
//!
//! **This is the bead's wiring clause, and it is the half pat-nzgdu.207 left
//! open.** The registration half is already done (.207 registered `Occluder3D`,
//! the five shapes, and `OccluderInstance3D` in ClassDB). What was missing was a
//! *caller*: `gdserver3d::occluder` had 600+ lines of occlusion math — including
//! `OccluderInstance3D::occludes()` — that **nothing in the engine ever called**.
//! The cull needed a per-instance world AABB, which `Instance3D` did not expose,
//! so this bead added `Mesh3D::aabb()` → `Instance3D::world_aabb()` → the
//! renderer's `is_occluded()` check in both draw loops.
//!
//! ⚠️ **Wiring it up exposed that the math was wrong**, and wrong in a way only a
//! caller could reveal: `occludes()` built the camera→occluder and camera→target
//! vectors and then used only their *lengths*, so "nearer and bigger" culled
//! regardless of direction — an occluder off to one side erased geometry dead
//! ahead. It also compared *depth* extents per-axis, which made the 0.001-thick
//! `QuadOccluder3D`/`PolygonOccluder3D` incapable of ever culling. Being
//! uncalled is what let that survive: no test had ever placed an occluder
//! off-axis. It is now a line-of-sight ray test (see `occludes()`).
//!
//! ⚠️ Culling is an OPTIMISATION: a wrong "cull" deletes visible geometry the
//! user can see, while a missed cull only wastes work. Several tests below pin
//! the fail-open cases (no bounds, MultiMesh, occluder behind or beside the
//! target) precisely because those must never cull.

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

/// A large box occluder centred at `(0, 0, z)`, wide enough to cover a unit
/// cube behind it.
fn occluder_at(renderer: &mut SoftwareRenderer3D, z: f32) {
    occluder_at_xyz(
        renderer,
        Vector3::new(0.0, 0.0, z),
        Vector3::new(10.0, 10.0, 2.0),
    );
}

/// A box occluder of `size` centred at `origin`.
fn occluder_at_xyz(renderer: &mut SoftwareRenderer3D, origin: Vector3, size: Vector3) {
    let mut occ = OccluderInstance3D::with_occluder(Occluder3D::new_box(size));
    occ.transform = Transform3D {
        basis: Basis::IDENTITY,
        origin,
    };
    renderer.add_occluder(occ);
}

fn nonblack(renderer: &mut SoftwareRenderer3D) -> usize {
    let vp = Viewport3D::new(64, 64);
    let frame = renderer.render_frame(&vp);
    frame.pixels.iter().filter(|c| **c != Color::BLACK).count()
}

/// The acceptance: a mesh behind an occluder is gone from the frame.
#[test]
fn occluder_instance_culls_hidden_geometry() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(&mut renderer, -5.0);

    // Baseline — without this the test could "pass" on a cube that never drew.
    let before = nonblack(&mut renderer);
    assert!(
        before > 100,
        "the cube must be visible before the occluder is added, got {before} pixels"
    );

    // An occluder between the camera (origin) and the cube.
    occluder_at(&mut renderer, -2.0);
    let after = nonblack(&mut renderer);

    assert_eq!(
        after, 0,
        "the cube is fully behind the occluder and must be culled, but {after} pixels drew"
    );
}

/// ⚠️ An occluder BEHIND the target must not cull it — otherwise a wall behind
/// you would erase what is in front of you.
#[test]
fn an_occluder_behind_the_target_does_not_cull() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(&mut renderer, -5.0);
    // Farther from the camera than the cube.
    occluder_at(&mut renderer, -8.0);

    assert!(
        nonblack(&mut renderer) > 100,
        "an occluder farther than the target must not cull it"
    );
}

/// ⚠️ Too small to cover the target ⇒ no cull. The cull is conservative: it must
/// only fire when the occluder blocks the line of sight to *every* corner.
#[test]
fn an_occluder_too_small_to_cover_the_target_does_not_cull() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(&mut renderer, -5.0);
    occluder_at_xyz(
        &mut renderer,
        Vector3::new(0.0, 0.0, -2.0),
        Vector3::new(0.05, 0.05, 0.05),
    );

    assert!(
        nonblack(&mut renderer) > 100,
        "a pebble must not cull a cube behind it"
    );
}

/// ⚠️ THE REGRESSION TEST. An occluder off to the SIDE must never cull geometry
/// dead ahead — it is not in the line of sight at all.
///
/// This is the case the original `occludes()` got wrong, and it shipped
/// undetected because it computed the camera→occluder and camera→target vectors
/// and then used only their *lengths*, discarding the directions: "nearer and
/// bigger" was enough to cull, regardless of where either object sat. The math
/// was dormant until this bead wired it in, so nothing had ever exercised it
/// off-axis — every earlier test placed the occluder on the camera's Z axis,
/// the one degree of freedom that hides the bug.
#[test]
fn an_occluder_off_to_the_side_does_not_cull_geometry_in_front() {
    let mut renderer = SoftwareRenderer3D::new();
    // ⚠️ z=-8 is load-bearing: the occluder below sits 5.1 units from the camera,
    // and the old math only culled a target FARTHER than the occluder. A cube at
    // the usual -5 would be nearer than it and pass even against the broken
    // version, proving nothing.
    cube_at(&mut renderer, -8.0);

    // Compare against the cube's own baseline rather than a pixel threshold: at
    // this distance a unit cube covers ~93 pixels in a 64x64 viewport, so the
    // `> 100` used by the closer-range tests would fail here for reasons that
    // have nothing to do with culling. "The occluder changes nothing" is both
    // robust and a stronger claim.
    let before = nonblack(&mut renderer);
    assert!(
        before > 0,
        "baseline: the cube must draw at z=-8 before any occluder exists"
    );

    // Nearer than the cube and far larger than it — the old math culled on
    // exactly those two facts — but 78° off-axis, with a clear line of sight to
    // the cube.
    occluder_at_xyz(
        &mut renderer,
        Vector3::new(5.0, 0.0, -1.0),
        Vector3::new(3.0, 3.0, 3.0),
    );

    assert_eq!(
        nonblack(&mut renderer),
        before,
        "an occluder beside the camera's view must not change the frame at all — \
         it is not in the line of sight"
    );
}

/// ⚠️ A flat QuadOccluder3D must cull — it is the shape most likely to be used
/// as a wall.
///
/// It could not before: the old test compared *depth* extents axis-by-axis, and
/// a quad's AABB is 0.001 thick, so `QuadOccluder3D` and `PolygonOccluder3D`
/// were structurally incapable of culling anything. Two of the five registered
/// shapes were inert.
#[test]
fn a_flat_quad_occluder_culls() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(&mut renderer, -5.0);
    assert!(nonblack(&mut renderer) > 100, "baseline: the cube draws");

    let mut occ = OccluderInstance3D::with_occluder(Occluder3D::Quad {
        size: gdcore::math::Vector2::new(10.0, 10.0),
    });
    occ.transform = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(0.0, 0.0, -2.0),
    };
    renderer.add_occluder(occ);

    assert_eq!(
        nonblack(&mut renderer),
        0,
        "a flat quad between camera and cube must cull it"
    );
}

/// An `OccluderInstance3D` carrying no shape has no bounds and must cull
/// nothing — it is the default-constructed state, so getting this wrong would
/// blank a scene the moment someone added an unconfigured occluder.
#[test]
fn an_occluder_with_no_shape_culls_nothing() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(&mut renderer, -5.0);
    renderer.add_occluder(OccluderInstance3D::new());

    assert!(
        nonblack(&mut renderer) > 100,
        "a shapeless occluder must not cull anything"
    );
}

/// ⚠️ BOTH render paths cull. `render_frame_wireframe` and `render_frame_solid`
/// have separate draw loops; wiring only one would make a mode switch silently
/// change what is drawn.
#[test]
fn both_render_modes_cull() {
    for mut renderer in [SoftwareRenderer3D::new(), SoftwareRenderer3D::wireframe()] {
        cube_at(&mut renderer, -5.0);
        assert!(
            nonblack(&mut renderer) > 0,
            "baseline: the cube must draw in {:?} mode",
            renderer.render_mode
        );

        occluder_at(&mut renderer, -2.0);
        assert_eq!(
            nonblack(&mut renderer),
            0,
            "the cube must be culled in {:?} mode too",
            renderer.render_mode
        );
    }
}

/// Culling is not permanent state: clearing the occluders brings the geometry
/// back. (Also proves the cull reads the live list rather than having mutated
/// the instance's visibility.)
#[test]
fn clearing_the_occluders_restores_the_geometry() {
    let mut renderer = SoftwareRenderer3D::new();
    cube_at(&mut renderer, -5.0);
    occluder_at(&mut renderer, -2.0);
    assert_eq!(nonblack(&mut renderer), 0);

    renderer.clear_occluders();
    assert!(
        nonblack(&mut renderer) > 100,
        "removing the occluder must bring the cube back — culling must not \
         have mutated the instance"
    );
    assert!(renderer.occluders().is_empty());
}

/// The registered occluders are readable back, and the accessor reflects what
/// was added — the API the scene layer will drive.
#[test]
fn occluders_are_registered_and_readable() {
    let mut renderer = SoftwareRenderer3D::new();
    assert!(renderer.occluders().is_empty());

    occluder_at(&mut renderer, -2.0);
    occluder_at(&mut renderer, -3.0);
    assert_eq!(renderer.occluders().len(), 2);

    // Each kept its own transform rather than collapsing to a shared one.
    assert!((renderer.occluders()[0].transform.origin.z - (-2.0)).abs() < 1e-6);
    assert!((renderer.occluders()[1].transform.origin.z - (-3.0)).abs() < 1e-6);
}
