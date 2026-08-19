//! pat-s3rrv.31 acceptance: a `.glb` referenced by a `MeshInstance3D` renders
//! its own geometry in the editor viewport, not the placeholder cube.
//!
//! Both halves of this already existed and never met. `gdresource`'s
//! `import_gltf` produces a `Resource`, and `mesh3d_from_gltf_resource`
//! (gdscene) converts one to a `Mesh3D` — but that converter's only caller sat
//! inside `RenderServer3DAdapter`, which has **no production callers at all**.
//! Meanwhile the editor's one file-backed loader, `mesh_from_file`, early
//! returned unless the path ended in `.obj`. So every glTF model in a scene fell
//! through to the 0.4-unit marker cube.
//!
//! ⚠️ The fixture geometry is load-bearing. The triangle is authored large and
//! square-on to the camera so it certainly covers pixels; a model that renders
//! to nothing would make "the glTF loaded" and "nothing was drawn" look
//! identical, which is the failure mode this test exists to distinguish.

use gdcore::math::Color;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::render_editor_3d_frame;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const W: u32 = 96;
const H: u32 = 72;

/// Wraps glTF JSON + its binary blob in a real `.glb` container, so the test
/// drives the same entry point a shipped asset would. Structure copied from
/// `gdscene/tests/gltf_anim_import.rs`, which proves this container parses.
fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut json_chunk = json.as_bytes().to_vec();
    while json_chunk.len() % 4 != 0 {
        json_chunk.push(b' ');
    }
    let mut bin_chunk = bin.to_vec();
    while bin_chunk.len() % 4 != 0 {
        bin_chunk.push(0);
    }
    let total = 12 + 8 + json_chunk.len() + 8 + bin_chunk.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes()); // "JSON"
    out.extend_from_slice(&json_chunk);
    out.extend_from_slice(&(bin_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E_4942u32.to_le_bytes()); // "BIN\0"
    out.extend_from_slice(&bin_chunk);
    out
}

/// One big triangle in the XY plane at z = 0, facing +Z (counter-clockwise from
/// the camera's side, so a back-face cull keeps it).
///
/// Deliberately ±2 units rather than a unit primitive: it must project to a
/// large, unmistakable area so the assertion is about the mesh loading rather
/// than about sub-pixel coverage.
fn triangle_glb() -> Vec<u8> {
    let mut bin = Vec::new();
    for p in [
        [-2.0f32, -2.0, 0.0],
        [2.0f32, -2.0, 0.0],
        [0.0f32, 2.0, 0.0],
    ] {
        for v in p {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    // ⚠️ Explicit indices, and BOTH windings of the same triangle.
    //
    // Two separate hazards, both of which render nothing and look identical to
    // "the loader never ran":
    //  1. `mesh3d_from_gltf_resource` treats a missing `indices` as an EMPTY
    //     list (`unwrap_or_default`), so an index-less mesh forms no triangles.
    //  2. The software rasterizer back-face culls on SCREEN-space winding
    //     (pat-s3rrv.30), and screen Y is flipped relative to world Y — so a
    //     triangle that is counter-clockwise in world XY is clockwise on screen
    //     and gets culled. Emitting 0,1,2 *and* 0,2,1 makes the fixture
    //     orientation-proof: whichever winding faces the camera survives.
    //
    // 6 × u16 at byte 36; u16 needs 2-byte alignment and 36 is even.
    for i in [0u16, 1, 2, 0, 2, 1] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    assert_eq!(bin.len(), 48, "fixture buffer layout changed");

    let json = r#"{
      "asset": {"version": "2.0"},
      "scene": 0,
      "scenes": [{"nodes": [0]}],
      "nodes": [{"name": "Tri", "mesh": 0}],
      "meshes": [{"name": "TriMesh", "primitives": [
        {"attributes": {"POSITION": 0}, "indices": 1}
      ]}],
      "buffers": [{"byteLength": 48}],
      "bufferViews": [
        {"buffer": 0, "byteOffset": 0,  "byteLength": 36},
        {"buffer": 0, "byteOffset": 36, "byteLength": 12}
      ],
      "accessors": [
        {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"},
        {"bufferView": 1, "componentType": 5123, "count": 6, "type": "SCALAR"}
      ]
    }"#;
    glb(json, &bin)
}

fn viewport() -> Viewport3D {
    Viewport3D::new(W, H)
}

fn render(tscn: &str) -> Vec<Color> {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    render_editor_3d_frame(&viewport(), &tree).pixels
}

fn differing(a: &[Color], b: &[Color]) -> usize {
    a.iter()
        .zip(b.iter())
        .filter(|(x, y)| {
            (x.r - y.r).abs() > 1e-3 || (x.g - y.g).abs() > 1e-3 || (x.b - y.b).abs() > 1e-3
        })
        .count()
}

/// **The acceptance test.**
#[test]
fn gltf_mesh_renders_in_viewport() {
    gdobject::class_db::register_3d_classes();

    let dir = tempfile::tempdir().expect("tempdir");
    let glb_path = dir.path().join("tri.glb");
    std::fs::write(&glb_path, triangle_glb()).expect("write glb");
    let glb_str = glb_path.to_string_lossy().to_string();

    // The scene under test: a MeshInstance3D pointing at the .glb on disk.
    let with_mesh = format!(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"

[node name="Model" type="MeshInstance3D" parent="."]
mesh = "{glb_str}"
"#
    );

    // The control: identical, minus the mesh reference. Proves the difference
    // comes from the model rather than from the camera or the light.
    let without_mesh = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"
"#;

    let framed = render(&with_mesh);
    let bare = render(without_mesh);

    // ⚠️ Fixture guard FIRST. Without it, a scene that renders nothing at all
    // makes every assertion below read as "the loader is broken" — which is
    // exactly how this test failed on its first run. The floor is shared by
    // both scenes, so if it does not draw, the camera/lighting setup is wrong
    // and nothing downstream means anything.
    let empty = render(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true
"#,
    );
    assert!(
        differing(&bare, &empty) > 0,
        "fixture check: the shared floor must render, else this camera sees \
         nothing and the mesh assertions below cannot distinguish a broken \
         loader from a broken scene"
    );

    let drawn = differing(&framed, &bare);
    assert!(
        drawn > 0,
        "the .glb's geometry must reach the frame — {drawn} pixels differed from \
         the same scene without the mesh. Zero means `mesh_from_file` still \
         rejects everything that is not .obj, so the model never loaded."
    );

    // …and it is the MODEL, not the placeholder. Expressed as a comparison
    // against the fallback rather than a coverage percentage: a magic threshold
    // would encode this camera's framing, and the claim under test is precisely
    // "the glTF geometry, not the marker cube".
    //
    // An unresolvable path takes the same `_ => BoxMesh::default()` fallback the
    // loader used to take for every glTF, so this is the exact before/after.
    let placeholder = render(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"

[node name="Model" type="MeshInstance3D" parent="."]
mesh = "res://definitely-missing.glb"
"#,
    );
    assert!(
        differing(&framed, &placeholder) > 0,
        "the frame must show the .glb's own geometry, not the placeholder cube a \
         missing mesh falls back to — the two rendered identically, which is what \
         it looks like when the glTF silently fails to load and the fallback runs."
    );
}
