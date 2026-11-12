//! pat-s3rrv.206 regression: a ReflectionProbe's captured cubemap must be laid
//! out the way `ReflectionCubemap::sample_direction` reads it.
//!
//! The capture rendered each face with an ordinary outward-looking camera and
//! stored the frame as-is, but a cubemap face is addressed with the standard
//! cubemap s/t table — mirrored against such a camera on every face, and with
//! its rows reversed on ±Y. So a red panel captured toward `(-1, 1, -1)` read
//! back from `(-1, 1, +1)`: every reflection showed the mirror-image half of the
//! scene, and `a_rough_surface_takes_less_reflection_than_a_mirror` (gdeditor)
//! could never see the captured wall in its mirror.
//!
//! Each case puts one small red panel off-axis (so a mirrored face would read
//! elsewhere) and checks the sampler finds it in its direction and NOT in the
//! direction a per-axis mirror would have put it.

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Basis, Transform3D};
use gdrender3d::SoftwareRenderer3D;
use gdserver3d::material::{CullMode, Material3D, ShadingMode};
use gdserver3d::reflection_probe::{ReflectionProbe, ReflectionProbeId};
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;

fn is_red(c: Color) -> bool {
    c.r > 0.5 && c.g < 0.2 && c.b < 0.2
}

/// Captures a probe at the origin with one red panel centred at `at`, facing
/// the probe, and returns the cubemap.
fn capture_with_panel_at(at: Vector3) -> gdserver3d::reflection_probe::ReflectionCubemap {
    let mut r = SoftwareRenderer3D::new();
    let id = r.create_instance();
    // A 1x1 quad (normal +Z) turned so its normal points back at the origin.
    r.set_mesh(id, gdserver3d::primitive_mesh::PlaneMesh::quad().generate());
    r.set_material(
        id,
        Material3D {
            albedo: Color::new(1.0, 0.0, 0.0, 1.0),
            shading_mode: ShadingMode::Unlit,
            // Orientation-proof: this test is about WHERE it lands, not which
            // side faces the probe.
            cull_mode: CullMode::Disabled,
            ..Default::default()
        },
    );
    let n = (at * -1.0).normalized();
    let helper = if n.y.abs() < 0.9 {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(1.0, 0.0, 0.0)
    };
    let x = helper.cross(n).normalized();
    let y = n.cross(x);
    r.set_transform(
        id,
        Transform3D {
            basis: Basis { x, y, z: n },
            origin: at,
        },
    );
    r.set_visible(id, true);
    let pid = ReflectionProbeId(1);
    r.add_reflection_probe(pid);
    r.update_reflection_probe(&ReflectionProbe::new(pid));
    r.capture_reflection_probes(&Viewport3D::new(32, 32), 32);
    r.reflection_probes()[0]
        .captured
        .as_ref()
        .expect("an Once probe captures on its first pass")
        .as_ref()
        .clone()
}

#[test]
fn a_captured_panel_reads_back_from_its_own_direction_on_every_face() {
    // One case per dominant axis, each skewed on BOTH other axes.
    let cases = [
        Vector3::new(3.0, 1.0, -1.2),
        Vector3::new(-3.0, 1.0, 1.2),
        Vector3::new(1.2, 3.0, -1.0),
        Vector3::new(1.2, -3.0, 1.0),
        Vector3::new(-1.2, 1.0, 3.0),
        Vector3::new(1.2, -1.0, -3.0),
    ];
    for at in cases {
        let cube = capture_with_panel_at(at);
        assert!(
            is_red(cube.sample_direction(at)),
            "the panel at {at:?} must read back from its own direction; got {:?}",
            cube.sample_direction(at)
        );
        // The same direction mirrored in each non-dominant axis lands on the
        // same face; a face stored mirrored would show the panel there instead.
        let (ax, ay, az) = (at.x.abs(), at.y.abs(), at.z.abs());
        let mirrors: Vec<Vector3> = if ax >= ay && ax >= az {
            vec![
                Vector3::new(at.x, -at.y, at.z),
                Vector3::new(at.x, at.y, -at.z),
            ]
        } else if ay >= az {
            vec![
                Vector3::new(-at.x, at.y, at.z),
                Vector3::new(at.x, at.y, -at.z),
            ]
        } else {
            vec![
                Vector3::new(-at.x, at.y, at.z),
                Vector3::new(at.x, -at.y, at.z),
            ]
        };
        for m in mirrors {
            assert!(
                !is_red(cube.sample_direction(m)),
                "the panel at {at:?} also reads back from its mirror image {m:?} — \
                 the captured face is stored flipped against the sampler"
            );
        }
    }
}
