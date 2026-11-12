//! pat-772py: a Decal does not project onto an instance whose `layers` share no
//! bit with its `cull_mask`, on the GPU path the editor's Play view uses.
//!
//! Godot tests `decal.mask & instance.layer_mask` per fragment
//! (`scene_forward_clustered.glsl:1563`). The 3D platformer's BlobShadow sets
//! `cull_mask = 1048573` (every layer but 2) and puts the Robot mesh on layer 2,
//! so the blob darkens the floor and never the robot standing in its 12-unit
//! box. Patina's screen-space decal pass read `Decal3D::cull_mask` off the node
//! and then never consulted it, so it drew a translucent column over the robot.
//!
//! # Three renders, so each claim has its own control
//!
//! No decal, a decal with every layer, and a decal excluding layer 2. The
//! all-layers render proves the decal reaches the box top at all, which makes
//! "the excluding render leaves the box top unchanged" a real claim and not a
//! decal that simply missed. The floor inside the decal must still change in the
//! excluding render, so the mask is not just switching the decal off.

#![cfg(feature = "gpu")]

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;
use gdrender3d::wgpu_pipeline::GpuRenderer3D;
use gdserver3d::decal::Decal3D;
use gdserver3d::instance::GeometryInstanceSettings;
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::material::Material3D;
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::viewport::Viewport3D;

const W: u32 = 128;
const H: u32 = 128;

fn camera() -> Transform3D {
    Transform3D {
        basis: gdcore::math3d::Basis::IDENTITY,
        origin: Vector3::new(0.0, 8.0, 0.01),
    }
    .looking_at(Vector3::ZERO, Vector3::new(0.0, 0.0, -1.0))
}

fn render(decal_mask: Option<u32>) -> Option<FrameData3D> {
    let Some(mut r) = GpuRenderer3D::new() else {
        eprintln!("SKIP: no wgpu adapter; pat-772py's decal mask cannot be exercised. Not a pass.");
        return None;
    };
    let mut light = Light3D::directional(Light3DId(1));
    light.direction = Vector3::new(0.0, -1.0, 0.0);
    r.add_light(light.id);
    r.update_light(&light);
    let white = Material3D {
        albedo: Color::new(1.0, 1.0, 1.0, 1.0),
        ..Default::default()
    };

    let floor = r.create_instance();
    r.set_mesh(floor, Mesh3D::plane(20.0));
    r.set_material(floor, white.clone());
    r.set_transform(floor, Transform3D::IDENTITY);
    r.set_visible(floor, true);

    // The "robot": a box on layer 2 only, standing inside the decal's box.
    let robot = r.create_instance();
    r.set_mesh(robot, Mesh3D::cube(1.0));
    r.set_material(robot, white);
    r.set_transform(
        robot,
        Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.5, 0.0)),
    );
    r.set_visible(robot, true);
    r.set_geometry_instance_settings(
        robot,
        GeometryInstanceSettings {
            layers: 2,
            ..Default::default()
        },
    );

    if let Some(mask) = decal_mask {
        let mut d = Decal3D::new();
        d.size = Vector3::new(4.0, 6.0, 4.0);
        d.modulate = Color::new(0.0, 0.0, 0.0, 1.0);
        d.albedo_mix = 1.0;
        d.upper_fade = 0.0;
        d.lower_fade = 0.0;
        d.cull_mask = mask;
        r.add_decal(d);
    }

    let mut vp = Viewport3D::new(W, H);
    vp.camera_transform = camera();
    Some(r.render_frame(&vp))
}

fn pixel_of(world: Vector3) -> usize {
    let v = camera().inverse().xform(world);
    let t = (std::f32::consts::FRAC_PI_4 * 0.5).tan();
    let nx = v.x / (-v.z * t);
    let ny = v.y / (-v.z * t);
    let x = ((nx + 1.0) * 0.5 * W as f32) as u32;
    let y = ((1.0 - ny) * 0.5 * H as f32) as u32;
    (y * W + x) as usize
}

fn lum(f: &FrameData3D, i: usize) -> f32 {
    let c = f.pixels[i];
    c.r + c.g + c.b
}

#[test]
fn a_decal_skips_an_instance_its_cull_mask_excludes() {
    let (Some(none), Some(all), Some(excl)) =
        (render(None), render(Some(u32::MAX)), render(Some(1048573)))
    else {
        return;
    };
    let box_top = pixel_of(Vector3::new(0.0, 1.0, 0.0));
    let floor_in = pixel_of(Vector3::new(1.5, 0.0, 1.5));

    assert!(
        lum(&all, box_top) < 0.5 * lum(&none, box_top),
        "control: an all-layers decal darkens the box top ({} vs {})",
        lum(&all, box_top),
        lum(&none, box_top)
    );
    assert!(
        (lum(&excl, box_top) - lum(&none, box_top)).abs() < 0.02,
        "cull_mask 1048573 excludes layer 2, so the box top must be untouched: \
         {} with the decal vs {} without",
        lum(&excl, box_top),
        lum(&none, box_top)
    );
    assert!(
        lum(&excl, floor_in) < 0.5 * lum(&none, floor_in),
        "the floor (layer 1) is still in the mask and must be darkened: {} vs {}",
        lum(&excl, floor_in),
        lum(&none, floor_in)
    );
}
