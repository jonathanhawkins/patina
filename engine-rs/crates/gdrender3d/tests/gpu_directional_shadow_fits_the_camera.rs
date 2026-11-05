//! pat-772py: the GPU path's DirectionalLight3D shadow map is fitted to the
//! camera, the way Godot's `_light_instance_setup_directional_shadow` fits it,
//! instead of a fixed 256-texel box spanning ±20 units around the world origin.
//!
//! The 3D platformer's stage reaches well past ±20 units, so most of it got no
//! shadow at all, and inside the box one texel was 0.16 world units, so the
//! shadows that did land were blocky. Godot fits the map to the camera frustum
//! out to `directional_shadow_max_distance` and filters it with a rotated
//! Vogel-disk PCF.
//!
//! # Differential, so nothing but the shadow is measured
//!
//! Each scene renders twice, with `shadow_enabled` on and off. Only the shadow
//! term differs, so every changed pixel is shadow. The assertions are on where
//! it lands, measured from the analytic shadow footprint of a box under a 45°
//! sun, and on how dark it is.
//!
//! The scene sits at x = 60, outside the legacy box. At HEAD that gives no
//! shadow at all, which `a_caster_far_from_the_origin_still_shadows` catches.

#![cfg(feature = "gpu")]

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;
use gdrender3d::wgpu_pipeline::GpuRenderer3D;
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::material::Material3D;
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::viewport::Viewport3D;

const W: u32 = 192;
const H: u32 = 144;
/// Far from the origin: the legacy map covered only |x|, |z| <= 20.
const X0: f32 = 60.0;

fn gpu_renderer() -> Option<GpuRenderer3D> {
    let g = GpuRenderer3D::new();
    if g.is_none() {
        eprintln!("SKIP: no wgpu adapter; pat-772py's GPU shadow cannot be exercised. Not a pass.");
    }
    g
}

fn camera() -> Transform3D {
    Transform3D {
        basis: gdcore::math3d::Basis::IDENTITY,
        origin: Vector3::new(X0 + 2.0, 9.0, 9.0),
    }
    .looking_at(
        Vector3::new(X0 + 2.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    )
}

/// A floor at y = 0 and a 2-unit box standing on it at (X0, 1, 0), lit by a
/// sun travelling along +X and down at 45°. The box's shadow on the floor is
/// exactly `x in [X0 - 1, X0 + 3], z in [-1, 1]`, of which `x > X0 + 1` is not
/// under the box.
fn render(shadows: bool) -> Option<FrameData3D> {
    let mut r = gpu_renderer()?;
    let mut light = Light3D::directional(Light3DId(1));
    light.direction = Vector3::new(1.0, -1.0, 0.0).normalized();
    light.energy = 1.0;
    light.shadow_enabled = shadows;
    r.add_light(light.id);
    r.update_light(&light);

    let white = Material3D {
        albedo: Color::new(1.0, 1.0, 1.0, 1.0),
        ..Default::default()
    };
    let floor = r.create_instance();
    r.set_mesh(floor, Mesh3D::plane(60.0));
    r.set_material(floor, white.clone());
    r.set_transform(
        floor,
        Transform3D::IDENTITY.translated(Vector3::new(X0, 0.0, 0.0)),
    );
    r.set_visible(floor, true);

    let caster = r.create_instance();
    r.set_mesh(caster, Mesh3D::cube(2.0));
    r.set_material(caster, white);
    r.set_transform(
        caster,
        Transform3D::IDENTITY.translated(Vector3::new(X0, 1.0, 0.0)),
    );
    r.set_visible(caster, true);

    let mut vp = Viewport3D::new(W, H);
    vp.camera_transform = camera();
    // Godot's default camera far; the shadow then spans the light's default
    // `directional_shadow_max_distance` of 100.
    vp.far = 4000.0;
    Some(r.render_frame(&vp))
}

/// The pixel a floor point projects to.
fn pixel_of(world: Vector3) -> (u32, u32) {
    let v = camera().inverse().xform(world);
    let t = (std::f32::consts::FRAC_PI_4 * 0.5).tan();
    let aspect = W as f32 / H as f32;
    let nx = v.x / (-v.z * t * aspect);
    let ny = v.y / (-v.z * t);
    (
        ((nx + 1.0) * 0.5 * W as f32) as u32,
        ((1.0 - ny) * 0.5 * H as f32) as u32,
    )
}

fn lum(f: &FrameData3D, (x, y): (u32, u32)) -> f32 {
    let c = f.pixels[(y * W + x) as usize];
    c.r + c.g + c.b
}

/// Darkening ratio (shadowed / unshadowed) at a floor point.
fn ratio(on: &FrameData3D, off: &FrameData3D, p: Vector3) -> f32 {
    let px = pixel_of(p);
    lum(on, px) / lum(off, px).max(1e-6)
}

#[test]
fn a_caster_far_from_the_origin_still_shadows() {
    let (Some(on), Some(off)) = (render(true), render(false)) else {
        return;
    };
    // Inside the footprint and not under the box: the sun's direct term is
    // gone, so only ambient remains.
    for x in [X0 + 1.6, X0 + 2.0, X0 + 2.4] {
        let r = ratio(&on, &off, Vector3::new(x, 0.0, 0.0));
        assert!(
            r < 0.5,
            "the floor at x={x} lies in the box's shadow, but shadowed/unshadowed \
             brightness is {r} (1.0 = no shadow at all: the map did not reach it)"
        );
    }
}

#[test]
fn the_shadow_ends_where_the_footprint_ends() {
    let (Some(on), Some(off)) = (render(true), render(false)) else {
        return;
    };
    // Outside the footprint by 0.4 units on both sides: lit. The old 0.16-unit
    // texels made the edge blocky but would pass this; what fails it is a
    // shadow detached from its caster (a bias too large, or the fit's depth
    // range wrong) or one smeared far past the edge.
    for p in [
        Vector3::new(X0 + 3.4, 0.0, 0.0),
        Vector3::new(X0 + 2.0, 0.0, 1.4),
        Vector3::new(X0 + 2.0, 0.0, -1.4),
    ] {
        let r = ratio(&on, &off, p);
        assert!(
            r > 0.95,
            "{p:?} is outside the shadow footprint but darkened to {r}"
        );
    }
    // And just inside the far edge: still shadowed, so the shadow reaches the
    // end of its footprint rather than stopping short.
    let r = ratio(&on, &off, Vector3::new(X0 + 2.6, 0.0, 0.6));
    assert!(
        r < 0.5,
        "just inside the footprint's corner: darkened only to {r}"
    );
}
