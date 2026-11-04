//! pat-s3rrv.122 regression: `should_cull` and `clip_to_screen` must agree
//! about which way is "front".
//!
//! # Why the existing unit tests did not catch this
//!
//! `render_modes`' own tests feed `should_cull` hand-written triangles and pass
//! whichever polarity the function has, because they were authored in ordinary
//! y-UP math coordinates and named `ccw`/`cw` accordingly. The renderer calls
//! `should_cull` with vertices straight out of `clip_to_screen`, whose y grows
//! DOWNWARD — so the composition was inverted while every part of it tested
//! green in isolation.
//!
//! `CullMode::Back` — Godot's default — therefore culled every FRONT face and
//! drew the far side of the mesh instead. On a closed convex opaque mesh that
//! is almost invisible: the silhouette is identical, and only the shading
//! normal and the UV direction change. It surfaced as
//! `gpu_shader_uniforms_and_textures_bound` reporting a UV gradient of the
//! right magnitude and the wrong SIGN against the wgpu path, which had the
//! convention right all along (`FrontFace::Ccw` + `Face::Back`).
//!
//! This test pins the SEAM rather than either side of it, so a future change to
//! either the screen mapping or the cull comparison has to keep them agreeing.
//!
//! ⚠️ CORRECTED convention. The account above judged "front" against
//! `Mesh3D::cube`, which was then wound counter-clockwise on its X/Z faces and
//! clockwise on its Y faces, and against wgpu's `FrontFace::Ccw`. Godot's front
//! face is CLOCKWISE (`doc/classes/ArrayMesh.xml:49`) — every primitive ported
//! from `primitive_meshes.cpp` is wound that way — so that rule culled a
//! `PlaneMesh` floor seen from above entirely. The cube is now clockwise on all
//! six faces, wgpu uses `FrontFace::Cw`, and the faces below are read from the
//! real `Mesh3D::cube` rather than retyped.

use gdcore::math::{Vector2, Vector3};
use gdrender3d::render_modes::{should_cull, signed_area, CullMode};
use gdrender3d::{clip_to_screen, ScreenVertex, VertexOutput};

const W: u32 = 64;
const H: u32 = 64;
/// `Viewport3D::new`'s default.
const FOV: f32 = std::f32::consts::FRAC_PI_4;

/// Projects a world-space point for the default camera: at the origin, looking
/// down -Z (`camera_transform: Transform3D::IDENTITY`), square viewport.
///
/// The clip Z is parked at 0 (mid-frustum) because this test is about winding,
/// not depth — but it must stay inside the near/far NDC range or
/// `clip_to_screen` rejects the vertex and the test would pass vacuously on
/// three `None`s.
fn project(world: Vector3) -> VertexOutput {
    let f = 1.0 / (FOV * 0.5).tan();
    VertexOutput {
        clip_position: [world.x * f, world.y * f, 0.0, -world.z],
        world_position: world,
        world_normal: Vector3::new(0.0, 0.0, 1.0),
        uv: [0.0, 0.0],
    }
}

fn to_screen(world: Vector3) -> ScreenVertex {
    clip_to_screen(&project(world), W, H)
        .expect("vertex is in front of the camera and inside the frustum")
}

fn screen_triangle(a: Vector3, b: Vector3, c: Vector3) -> (Vector2, Vector2, Vector2) {
    let (sa, sb, sc) = (to_screen(a), to_screen(b), to_screen(c));
    (
        Vector2::new(sa.x, sa.y),
        Vector2::new(sb.x, sb.y),
        Vector2::new(sc.x, sc.y),
    )
}

/// The first triangle of `Mesh3D::cube(2.0)` whose vertex normal is `normal`,
/// with the cube moved to z = -4, projected to screen — read from the REAL mesh
/// rather than retyped, so a change to the cube's winding is judged here too.
///
/// ⚠️ This used to hardcode the cube's +Z face as `(-1,-1) (1,-1) (1,1)` —
/// counter-clockwise from outside — and pin that as the front face. Godot's
/// front face is CLOCKWISE (`doc/classes/ArrayMesh.xml:49`), and every primitive
/// ported from `primitive_meshes.cpp` is wound that way, so the pinned
/// convention made each of them inside-out (a `PlaneMesh` floor seen from above
/// was culled entirely). The cube is now CW on all six faces.
fn cube_face(normal: Vector3) -> (Vector2, Vector2, Vector2) {
    let cube = gdserver3d::mesh::Mesh3D::cube(2.0);
    let tri = cube
        .indices
        .chunks(3)
        .find(|t| (cube.normals[t[0] as usize] - normal).length() < 1e-6)
        .expect("the cube has a face with this normal");
    let at = |i: u32| cube.vertices[i as usize] + Vector3::new(0.0, 0.0, -4.0);
    screen_triangle(at(tri[0]), at(tri[1]), at(tri[2]))
}

/// The camera-facing (+Z) face of the cube.
fn front_face() -> (Vector2, Vector2, Vector2) {
    cube_face(Vector3::new(0.0, 0.0, 1.0))
}

/// The opposite (-Z) face of the same cube — the one hidden behind it.
fn back_face() -> (Vector2, Vector2, Vector2) {
    cube_face(Vector3::new(0.0, 0.0, -1.0))
}

/// ⚠️ Asserted first: every other reading here is meaningless if the two faces
/// project to the same winding.
#[test]
fn the_two_faces_of_a_cube_have_opposite_screen_winding() {
    let (a, b, c) = front_face();
    let front_area = signed_area(a, b, c);
    let (a, b, c) = back_face();
    let back_area = signed_area(a, b, c);

    assert!(
        front_area != 0.0 && back_area != 0.0,
        "a face projected degenerate (front {front_area}, back {back_area}) — \
         the fixture is wrong, not the cull"
    );
    assert!(
        front_area * back_area < 0.0,
        "the camera-facing and away-facing cube faces must project to OPPOSITE \
         signed areas (front {front_area}, back {back_area})"
    );
}

/// The sign itself, stated once so the reason for `should_cull`'s comparisons
/// is discoverable from a test rather than only from a comment.
#[test]
fn a_camera_facing_triangle_has_positive_screen_area() {
    let (a, b, c) = front_face();
    let area = signed_area(a, b, c);
    assert!(
        area > 0.0,
        "screen Y grows downward, so a Godot front face (CLOCKWISE as the camera \
         sees it, `ArrayMesh.xml:49`) has a POSITIVE signed area — got {area}"
    );
}

/// **The regression.** Under Godot's default `CULL_BACK`, the face the camera
/// can see must survive and the one behind it must not.
#[test]
fn default_back_face_culling_keeps_the_camera_facing_face() {
    let (a, b, c) = front_face();
    assert!(
        !should_cull(a, b, c, CullMode::Back),
        "CULL_BACK culled the CAMERA-FACING face — this is the pat-s3rrv.122 \
         inversion: the renderer then draws the far side of every mesh, which \
         mirrors its UVs and lights it by a normal pointing away from the camera"
    );

    let (a, b, c) = back_face();
    assert!(
        should_cull(a, b, c, CullMode::Back),
        "CULL_BACK kept the away-facing face"
    );
}

/// `CULL_FRONT` is the exact mirror. Pinned because the bug was a swapped pair
/// of comparisons, and a fix that corrected only `Back` would leave `Front`
/// culling nothing at all.
#[test]
fn cull_front_is_the_exact_mirror_of_cull_back() {
    for (label, tri) in [("front", front_face()), ("back", back_face())] {
        let (a, b, c) = tri;
        assert_ne!(
            should_cull(a, b, c, CullMode::Back),
            should_cull(a, b, c, CullMode::Front),
            "the {label} face is culled by both modes or neither; \
             CULL_BACK and CULL_FRONT must disagree on every non-degenerate face"
        );
        assert!(
            !should_cull(a, b, c, CullMode::Disabled),
            "CULL_DISABLED must keep the {label} face"
        );
    }
}
