//! pat-3u1wu (metals): a ReflectionProbe's captured cubemap holds the scene's
//! LINEAR radiance, as Godot's reflection atlas does — not the tonemapped,
//! sRGB-encoded picture a camera shows.
//!
//! The capture rendered each face through the full camera post-process, so a
//! surface of linear 0.214 (sRGB 0.5) was stored as 0.5 and both renderers
//! multiplied that straight into their linear shading sum. Every reflection
//! came out lifted; the 3D platformer's `player_gray.tres` robot (metallic 1,
//! no diffuse, all of its colour is reflection) drew light grey where Godot
//! shows it near-black reflecting the dark level walls.

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Basis, Transform3D};
use gdrender3d::SoftwareRenderer3D;
use gdserver3d::environment::{Environment3D, ToneMapper};
use gdserver3d::material::{CullMode, Material3D, ShadingMode};
use gdserver3d::reflection_probe::{ReflectionProbe, ReflectionProbeId};
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;

/// Captures a probe at the origin inside a closed unlit box of `albedo`
/// (authored sRGB, like a `.tscn` colour), and samples it straight ahead.
fn captured_wall(albedo: Color, env: Option<Environment3D>) -> Color {
    let mut r = SoftwareRenderer3D::new();
    let id = r.create_instance();
    r.set_mesh(id, gdserver3d::mesh::Mesh3D::cube(4.0));
    r.set_material(
        id,
        Material3D {
            albedo,
            shading_mode: ShadingMode::Unlit,
            cull_mode: CullMode::Disabled,
            ..Default::default()
        },
    );
    r.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::ZERO,
        },
    );
    r.set_visible(id, true);
    let pid = ReflectionProbeId(1);
    r.add_reflection_probe(pid);
    r.update_reflection_probe(&ReflectionProbe::new(pid));
    let mut vp = Viewport3D::new(32, 32);
    vp.environment = env;
    r.capture_reflection_probes(&vp, 16);
    r.reflection_probes()[0]
        .captured
        .as_ref()
        .expect("a Once probe captures on its first pass")
        .sample_direction(Vector3::new(0.0, 0.0, -1.0))
}

#[test]
fn a_probe_captures_linear_radiance_not_the_encoded_picture() {
    // sRGB 0.5 authored == linear 0.2140 (the standard piecewise EOTF).
    let linear = gdcore::math::srgb_to_linear_channel(0.5);
    let c = captured_wall(Color::new(0.5, 0.5, 0.5, 1.0), None);
    assert!(
        (c.r - linear).abs() < 0.01,
        "the probe must store the wall's LINEAR radiance {linear:.4}; got {:.4} \
         (0.5 means the capture went through the camera's sRGB encode)",
        c.r
    );
}

#[test]
fn a_probe_capture_is_not_tonemapped_by_the_camera_environment() {
    // The platformer's environment tonemaps with AgX. A capture taken through
    // that curve stores a different value for the same wall; Godot applies
    // tonemapping once, to the final picture.
    let env = Environment3D {
        tone_mapper: ToneMapper::Agx,
        ..Default::default()
    };
    let plain = captured_wall(Color::new(0.5, 0.5, 0.5, 1.0), None);
    let agx = captured_wall(Color::new(0.5, 0.5, 0.5, 1.0), Some(env));
    assert!(
        (plain.r - agx.r).abs() < 1e-4,
        "the environment's tonemapper must not shape a probe capture: \
         linear {:.4} vs AgX-env {:.4}",
        plain.r,
        agx.r
    );
}
