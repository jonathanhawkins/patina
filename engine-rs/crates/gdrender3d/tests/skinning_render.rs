//! Render-level acceptance tests for skeletal skinning (pat-u3l8c.33).
//!
//! These drive the **software** render path end-to-end: a rigged `Mesh3D`
//! (4-influence `bone_weights`/`bone_indices`) is deformed by
//! `gdserver3d::skin::skin_mesh` using per-bone poses, then rasterized by
//! `SoftwareRenderer3D`. The tests assert that changing a bone's pose visibly
//! changes the rendered frame — i.e. animated characters actually deform.

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Basis, Transform3D};
use gdrender3d::renderer::FrameBuffer3D;
use gdrender3d::test_adapter::{capture_frame_3d, count_visible_pixels};
use gdrender3d::SoftwareRenderer3D;
use gdserver3d::material::{Material3D, ShadingMode};
use gdserver3d::mesh::{Mesh3D, PrimitiveType};
use gdserver3d::server::RenderingServer3D;
use gdserver3d::skin::{skin_mesh, Skin};
use gdserver3d::viewport::Viewport3D;

const W: u32 = 64;
const H: u32 = 64;

/// A rigged quad centered on the origin. The two bottom vertices follow bind 0
/// and the two top vertices follow bind 1 (single full-weight influence each),
/// so posing bind 1 shears the top of the quad while the bottom stays put.
fn rigged_quad() -> Mesh3D {
    let mut mesh = Mesh3D::new(PrimitiveType::Triangles);
    mesh.vertices = vec![
        Vector3::new(-0.4, -0.5, 0.0), // 0 bottom-left  -> bind 0
        Vector3::new(0.4, -0.5, 0.0),  // 1 bottom-right -> bind 0
        Vector3::new(0.4, 0.5, 0.0),   // 2 top-right    -> bind 1
        Vector3::new(-0.4, 0.5, 0.0),  // 3 top-left     -> bind 1
    ];
    mesh.normals = vec![Vector3::new(0.0, 0.0, 1.0); 4];
    mesh.uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    // Clockwise as the camera (on +Z, looking down -Z) sees it: Godot's front
    // face (`doc/classes/ArrayMesh.xml:49`), so CULL_BACK keeps it. The
    // counter-clockwise `0,1,2 / 0,2,3` only rendered while the cull had the
    // convention inverted.
    mesh.indices = vec![0, 2, 1, 0, 3, 2];
    mesh.bone_weights = vec![[1.0, 0.0, 0.0, 0.0]; 4];
    mesh.bone_indices = vec![[0, 0, 0, 0], [0, 0, 0, 0], [1, 0, 0, 0], [1, 0, 0, 0]];
    mesh
}

/// Two identity binds (bind 0 -> bone 0, bind 1 -> bone 1).
fn identity_skin() -> Skin {
    let mut skin = Skin::new();
    skin.add_bind(0, Transform3D::IDENTITY);
    skin.add_bind(1, Transform3D::IDENTITY);
    skin
}

fn red_unlit() -> Material3D {
    Material3D {
        albedo: Color::new(1.0, 0.0, 0.0, 1.0),
        shading_mode: ShadingMode::Unlit,
        ..Material3D::default()
    }
}

/// Renders a single mesh at z = -5 (in front of the default camera) and
/// returns the captured framebuffer.
fn render_mesh(mesh: Mesh3D) -> FrameBuffer3D {
    let mut renderer = SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, mesh);
    renderer.set_material(id, red_unlit());
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );
    let vp = Viewport3D::new(W, H);
    capture_frame_3d(&mut renderer, &vp)
}

/// Counts non-background pixels in the right half of the frame (`x >= W/2`).
fn right_half_visible(fb: &FrameBuffer3D) -> usize {
    let mut count = 0usize;
    for y in 0..fb.height {
        for x in (fb.width / 2)..fb.width {
            if fb.get_pixel(x, y) != Color::BLACK {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn skinning_deforms_rendered_mesh_per_bone_pose() {
    let mesh = rigged_quad();
    let skin = identity_skin();

    // Rest: both bones identity -> skinning is a no-op.
    let rest_mesh = skin_mesh(
        &mesh,
        &skin,
        &[Transform3D::IDENTITY, Transform3D::IDENTITY],
    );
    // Posed: bone 1 slides +X, shearing the top of the quad to the right.
    let posed_mesh = skin_mesh(
        &mesh,
        &skin,
        &[
            Transform3D::IDENTITY,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.5, 0.0, 0.0),
            },
        ],
    );

    // Skinning at rest must equal the original geometry.
    assert_eq!(
        rest_mesh.vertices, mesh.vertices,
        "rest skin changed geometry"
    );
    // Posing must move the two top vertices and leave the bottom two.
    assert_eq!(posed_mesh.vertices[0], mesh.vertices[0]);
    assert_eq!(posed_mesh.vertices[1], mesh.vertices[1]);
    assert_ne!(posed_mesh.vertices[2], mesh.vertices[2]);
    assert_ne!(posed_mesh.vertices[3], mesh.vertices[3]);

    let fb_rest = render_mesh(rest_mesh);
    let fb_posed = render_mesh(posed_mesh);

    let vis_rest = count_visible_pixels(&fb_rest);
    let vis_posed = count_visible_pixels(&fb_posed);
    assert!(vis_rest > 20, "rest quad should render ({vis_rest} px)");
    assert!(vis_posed > 20, "posed quad should render ({vis_posed} px)");

    // The rendered frames must differ — the pose changed what was drawn.
    assert!(
        fb_rest.pixels != fb_posed.pixels,
        "posing a bone did not change the rendered frame"
    );

    // The deformation shears the top of the quad to the right, so the posed
    // frame paints strictly more of its right half than the rest frame does.
    let right_rest = right_half_visible(&fb_rest);
    let right_posed = right_half_visible(&fb_posed);
    assert!(
        right_posed > right_rest,
        "posed render should shift geometry rightward (rest={right_rest}, posed={right_posed})"
    );
}

#[test]
fn four_influence_skinning_renders() {
    // A single quad whose every vertex is blended equally across all four
    // binds, exercising the full 4-influence path (weights + indices) through
    // skinning and the renderer. With identity binds/poses the blend is a
    // no-op, so the quad must still render as a coherent, visible shape.
    let mut mesh = Mesh3D::new(PrimitiveType::Triangles);
    mesh.vertices = vec![
        Vector3::new(-0.4, -0.4, 0.0),
        Vector3::new(0.4, -0.4, 0.0),
        Vector3::new(0.4, 0.4, 0.0),
        Vector3::new(-0.4, 0.4, 0.0),
    ];
    mesh.normals = vec![Vector3::new(0.0, 0.0, 1.0); 4];
    mesh.uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    // Clockwise as the camera (on +Z, looking down -Z) sees it: Godot's front
    // face (`doc/classes/ArrayMesh.xml:49`), so CULL_BACK keeps it. The
    // counter-clockwise `0,1,2 / 0,2,3` only rendered while the cull had the
    // convention inverted.
    mesh.indices = vec![0, 2, 1, 0, 3, 2];
    // Every vertex: quarter weight on each of binds 0,1,2,3.
    mesh.bone_weights = vec![[0.25, 0.25, 0.25, 0.25]; 4];
    mesh.bone_indices = vec![[0, 1, 2, 3]; 4];
    assert!(mesh.is_skinned());

    let mut skin = Skin::new();
    for b in 0..4 {
        skin.add_bind(b, Transform3D::IDENTITY);
    }
    assert_eq!(skin.bind_count(), 4);

    // All identity poses -> the blend is a no-op, quad renders at rest.
    let poses = vec![Transform3D::IDENTITY; 4];
    let skinned = skin_mesh(&mesh, &skin, &poses);
    for (a, b) in mesh.vertices.iter().zip(skinned.vertices.iter()) {
        assert!(
            (*a - *b).length() < 1e-4,
            "identity 4-influence blend moved a vertex"
        );
    }

    let fb = render_mesh(skinned);
    assert!(
        count_visible_pixels(&fb) > 20,
        "4-influence skinned quad should render visibly"
    );
}
