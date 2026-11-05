//! 3D Play render time: when only a few shadow casters move over a static
//! level, the GPU path rewrites just their texel rectangles in the shadow
//! atlas it already holds — and the frame is bit-identical to a renderer that
//! uploads the whole atlas fresh.
//!
//! Before this, every frame where a caster moved (the 3D platformer's coins
//! spin, so: every frame) re-created and re-uploaded the whole camera-fitted
//! 2048² f32 atlas, 16 MB, and copied the 16 MB static base on the CPU first.

#![cfg(feature = "gpu")]

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Basis, Transform3D};
use gdrender3d::wgpu_pipeline::GpuRenderer3D;
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::material::Material3D;
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;

const W: u32 = 160;
const H: u32 = 120;

/// A floor of static tiles, a body that moves every frame and a spinning
/// "coin", as a running game re-adds them every frame.
fn build(r: &mut GpuRenderer3D, frame: u32) {
    r.reset_scene();
    let mut add = |mesh: Mesh3D, t: Transform3D| {
        let inst = r.create_instance();
        r.set_mesh(inst, mesh);
        r.set_material(
            inst,
            Material3D {
                albedo: Color::WHITE,
                ..Default::default()
            },
        );
        r.set_transform(inst, t);
        r.set_visible(inst, true);
    };
    for x in -4..=4 {
        for z in -8..=0 {
            add(
                Mesh3D::cube(0.95),
                Transform3D::IDENTITY.translated(Vector3::new(x as f32, -1.5, z as f32 - 2.0)),
            );
        }
    }
    let t = frame as f32 * 0.15;
    add(
        Mesh3D::cube(0.6),
        Transform3D::IDENTITY.translated(Vector3::new(t.sin() * 2.0, -0.3, -5.0)),
    );
    add(
        Mesh3D::cube(0.4),
        Transform3D {
            basis: Basis::from_euler(Vector3::new(0.0, t * 2.0, 0.0)),
            origin: Vector3::new(1.5, 0.2, -4.0),
        },
    );
    let mut sun = Light3D::directional(Light3DId(1));
    sun.direction = Vector3::new(-0.35, -1.0, -0.25).normalized();
    sun.shadow_enabled = true;
    r.add_light(sun.id);
    r.update_light(&sun);
}

fn viewport() -> Viewport3D {
    let mut vp = Viewport3D::new(W, H);
    vp.camera_transform = Transform3D::IDENTITY.translated(Vector3::new(0.0, 1.5, 2.0));
    vp
}

/// truck_town's shape: a sun, a shadowed spot light that moves with the car
/// every frame (a headlight), and an omni street lamp that the car never gets
/// near.
fn build_town(r: &mut GpuRenderer3D, frame: u32) {
    build(r, frame);
    let t = frame as f32 * 0.15;
    let mut head = Light3D::spot(Light3DId(2), Vector3::ZERO, Vector3::new(0.0, 0.0, -1.0));
    head.position = Vector3::new(t.sin() * 2.0, 0.2, -4.4);
    head.direction = Vector3::new(0.0, -0.3, -1.0).normalized();
    head.range = 12.0;
    head.shadow_enabled = true;
    r.add_light(head.id);
    r.update_light(&head);
    let mut lamp = Light3D::point(Light3DId(3), Vector3::ZERO);
    lamp.position = Vector3::new(-3.5, 0.5, -9.0);
    lamp.range = 3.0;
    lamp.shadow_enabled = true;
    r.add_light(lamp.id);
    r.update_light(&lamp);
}

/// A moving headlight re-rasterizes its own map only: the sun's base and the
/// lamp's cubemap are kept, and every frame still equals a fresh full render.
#[test]
fn a_moving_spot_light_keeps_the_suns_base_and_the_lamps_cubemap() {
    let Some(mut live) = GpuRenderer3D::new() else {
        eprintln!("SKIP: no wgpu adapter; this is a SKIP, not a pass");
        return;
    };
    let vp = viewport();
    let clear = Color::new(0.0, 0.0, 0.0, 1.0);
    let mut settled = None;
    let mut omni_builds = 0;
    for frame in 0..44u32 {
        build_town(&mut live, frame);
        let before = live.shadow_upload_texels();
        let builds_before = live.omni_cube_builds();
        let (_, _, bytes) = live.render_editor_color_bgra(&vp, clear);
        if frame >= 38 {
            settled = Some(
                settled
                    .unwrap_or(0)
                    .max(live.shadow_upload_texels() - before),
            );
            omni_builds += live.omni_cube_builds() - builds_before;
        }
        if frame == 43 {
            let mut fresh = GpuRenderer3D::new().expect("adapter");
            build_town(&mut fresh, frame);
            let (_, _, expected) = fresh.render_editor_color_bgra(&vp, clear);
            let differing = bytes
                .chunks_exact(4)
                .zip(expected.chunks_exact(4))
                .filter(|(a, b)| a != b)
                .count();
            assert_eq!(
                differing, 0,
                "frame {frame}: {differing} pixels differ from a full render"
            );
        }
    }
    assert_eq!(
        omni_builds, 0,
        "the lamp's cubemap was rendered again though nothing in its range moved"
    );
    let settled = settled.unwrap();
    assert!(
        settled < 2048 * 2048 / 4,
        "a frame where the headlight moved uploaded {settled} shadow texels: the sun's whole map went up again"
    );
}

#[test]
fn moving_casters_upload_their_rectangles_and_draw_the_same_frame() {
    let Some(mut live) = GpuRenderer3D::new() else {
        eprintln!("SKIP: no wgpu adapter; this is a SKIP, not a pass");
        return;
    };
    let vp = viewport();
    let clear = Color::new(0.0, 0.0, 0.0, 1.0);
    // Past the 30 calls a caster needs to join the static base.
    let mut checked = 0;
    let mut settled_texels = None;
    for frame in 0..48u32 {
        build(&mut live, frame);
        let before = live.shadow_upload_texels();
        let (_, _, bytes) = live.render_editor_color_bgra(&vp, clear);
        let uploaded = live.shadow_upload_texels() - before;
        if frame >= 40 {
            settled_texels = Some(settled_texels.unwrap_or(0).max(uploaded));
        }
        if frame == 41 || frame == 47 {
            // A fresh renderer has no base and no texture: it rasterizes and
            // uploads everything for this scene state.
            let mut fresh = GpuRenderer3D::new().expect("adapter");
            build(&mut fresh, frame);
            let (_, _, expected) = fresh.render_editor_color_bgra(&vp, clear);
            assert!(
                fresh.shadow_upload_texels() >= 2048 * 2048 / 4,
                "the fresh renderer uploads a whole atlas"
            );
            let differing = bytes
                .chunks_exact(4)
                .zip(expected.chunks_exact(4))
                .filter(|(a, b)| a != b)
                .count();
            assert_eq!(
                differing, 0,
                "frame {frame}: {differing} pixels differ from a full upload"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 2);
    let settled = settled_texels.unwrap();
    assert!(
        settled > 0,
        "the moving casters must change the atlas every frame"
    );
    assert!(
        settled * 16 < 2048 * 2048,
        "a frame where two small casters moved uploaded {settled} shadow texels — the whole atlas"
    );
}
