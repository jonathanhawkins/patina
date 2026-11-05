//! pat-414k3: the editor / Play colour path (a) skips draws wholly outside the
//! camera's clip volume without changing a pixel, and (b) draws in instance
//! creation order, so a frame is the same in every process.
//!
//! The official 3D platformer has over 3000 GridMap cells, most of them off
//! screen at any moment; every one used to cost a uniform buffer, a bind group
//! and a draw call. And the draws walked a `HashMap`, whose order is seeded per
//! process, so two coplanar surfaces resolved differently from one editor run
//! to the next — no whole-frame comparison of the renderer could hold.

#![cfg(feature = "gpu")]

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;
use gdrender3d::wgpu_pipeline::GpuRenderer3D;
use gdserver3d::material::{Material3D, ShadingMode};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;

const W: u32 = 64;
const H: u32 = 64;

fn add_cube(r: &mut GpuRenderer3D, at: Vector3, color: Color) {
    let inst = r.create_instance();
    r.set_mesh(inst, Mesh3D::cube(0.5));
    r.set_material(
        inst,
        Material3D { albedo: color, shading_mode: ShadingMode::Unlit, ..Default::default() },
    );
    r.set_transform(inst, Transform3D::IDENTITY.translated(at));
    r.set_visible(inst, true);
}

/// The default viewport camera sits at the origin looking down -Z.
fn offscreen_positions() -> Vec<Vector3> {
    let mut out = Vec::new();
    for i in 0..10 {
        let k = i as f32;
        out.push(Vector3::new(0.0, 0.0, 5.0 + k)); // behind the camera
        out.push(Vector3::new(-60.0 - k, 0.0, -2.5)); // far left
        out.push(Vector3::new(0.0, 60.0 + k, -2.5)); // far above
        out.push(Vector3::new(0.0, 0.0, -9000.0 - k)); // beyond the far plane
        out.push(Vector3::new(40.0 + k, -40.0, -5.0)); // far right, below
    }
    out
}

#[test]
fn draws_outside_the_view_are_culled_and_the_frame_is_unchanged() {
    let Some(mut r) = GpuRenderer3D::new() else {
        eprintln!("SKIP: no wgpu adapter; this is a SKIP, not a pass");
        return;
    };
    let vp = Viewport3D::new(W, H);
    let clear = Color::new(0.0, 0.0, 0.0, 1.0);
    let red = Color::new(1.0, 0.0, 0.0, 1.0);

    // Just the visible cube, plus one straddling the left edge of the view:
    // partly on screen, so it must NOT be culled.
    let visible = |r: &mut GpuRenderer3D| {
        add_cube(r, Vector3::new(0.0, 0.0, -2.5), red);
        add_cube(r, Vector3::new(-1.05, 0.0, -2.5), Color::new(0.0, 1.0, 0.0, 1.0));
    };
    r.reset_scene();
    visible(&mut r);
    let (_, _, alone) = r.render_editor_color_bgra(&vp, clear);
    assert_eq!(r.editor_last_draw_count(), 2);
    assert!(
        alone.chunks_exact(4).any(|p| p[1] > 200 && p[2] < 50),
        "the edge-straddling green cube must be partly in view, or this test proves nothing"
    );

    r.reset_scene();
    visible(&mut r);
    for p in offscreen_positions() {
        add_cube(&mut r, p, Color::new(0.0, 0.0, 1.0, 1.0));
    }
    let (_, _, with_offscreen) = r.render_editor_color_bgra(&vp, clear);
    assert_eq!(
        r.editor_last_draw_count(),
        2,
        "the 50 cubes outside the clip volume must not be drawn"
    );
    assert!(with_offscreen == alone, "culling must not change a single pixel");
}

#[test]
fn coplanar_draws_resolve_in_creation_order_in_every_renderer() {
    // Sixteen cubes at the SAME place, each its own colour. Their faces tie
    // in depth, so the depth test (`Less`) keeps whichever is drawn first.
    let colors: Vec<Color> = (0..16)
        .map(|i| Color::new((i as f32 + 1.0) / 16.0, 0.0, 1.0 - i as f32 / 16.0, 1.0))
        .collect();
    let vp = Viewport3D::new(W, H);
    let clear = Color::new(0.0, 0.0, 0.0, 1.0);
    let mut frames = Vec::new();
    // Fresh renderers: each `HashMap` of instances gets its own random seed.
    for _ in 0..3 {
        let Some(mut r) = GpuRenderer3D::new() else {
            eprintln!("SKIP: no wgpu adapter; this is a SKIP, not a pass");
            return;
        };
        r.reset_scene();
        for c in &colors {
            add_cube(&mut r, Vector3::new(0.0, 0.0, -2.5), *c);
        }
        let (_, _, frame) = r.render_editor_color_bgra(&vp, clear);
        frames.push(frame);
    }
    // The centre pixel shows the FIRST-created cube's colour.
    let center = ((H / 2 * W + W / 2) * 4) as usize;
    for f in &frames {
        let (b, r) = (f[center], f[center + 2]);
        let (fr, fb) = (colors[0].r, colors[0].b);
        assert!(
            (r as f32 / 255.0 - fr).abs() < 0.08 && (b as f32 / 255.0 - fb).abs() < 0.08,
            "centre is r={r} b={b}; the first-created cube is r={fr:.2} b={fb:.2}"
        );
    }
    assert!(frames.windows(2).all(|p| p[0] == p[1]), "every renderer draws the same frame");
}
