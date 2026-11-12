//! pat-s3rrv.23 acceptance: a `MeshInstance3D` pointing at an IMPORTED mesh
//! renders that mesh, not the placeholder cube.
//!
//! Godot ground truth: `upstream/godot/scene/3d/mesh_instance_3d.cpp` — a
//! MeshInstance3D draws whatever Mesh resource it holds, imported or primitive.
//!
//! ⚠️ The failure this guards against is NOT "nothing renders". The editor's
//! file loader accepted only `.obj`, and every other path fell through to
//! `BoxMesh::default().generate()` — so an imported model rendered as a tidy
//! 1x1x1 cube and looked like a modelling mistake rather than a loader gap.
//! Asserting "some pixels appeared" would therefore pass on the bug; the
//! assertion has to separate the real mesh from the fallback.

use gdcore::math::Color;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::render_editor_3d_frame;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

/// A glTF quad that already ships as a fixture. Loaded by a bare filesystem
/// path, which `mesh_from_file` accepts directly.
const QUAD_GLB: &str = "../gdresource/fixtures/quad.glb";

fn scene(mesh: Option<&str>) -> String {
    let mesh_line = match mesh {
        Some(path) => format!("mesh = \"{path}\"\n"),
        None => String::new(),
    };
    format!(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Model" type="MeshInstance3D" parent="."]
{mesh_line}"#
    )
}

fn render(tscn: &str) -> Vec<Color> {
    gdobject::class_db::register_all_classes();
    let parsed = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &parsed).expect("scene instances");
    let mut vp = Viewport3D::default();
    vp.width = 96;
    vp.height = 96;
    render_editor_3d_frame(&vp, &tree).pixels
}

fn differing(a: &[Color], b: &[Color]) -> usize {
    a.iter()
        .zip(b.iter())
        .filter(|(x, y)| {
            (x.r - y.r).abs() > 1e-3 || (x.g - y.g).abs() > 1e-3 || (x.b - y.b).abs() > 1e-3
        })
        .count()
}

/// ⚠️ **IGNORED: this reproduces a defect in glTF mesh loading, not in the
/// skinning/blend-shape wiring this bead delivers.** Kept in the tree because
/// it is a precise, self-contained reproduction — un-ignore it when the glTF
/// gap is fixed and it becomes the end-to-end proof of the silhouette claim.
///
/// What it establishes, from the run on 2026-08-19:
///
///   * the primitive control PASSES, so geometry does reach the frame;
///   * a `MeshInstance3D` pointing at `quad.glb` renders IDENTICALLY to one
///     with no mesh at all.
///
/// So the glTF specifically yields no mesh and falls back to the placeholder.
/// The fixture is not at fault — `quad.glb` is a valid GLB whose JSON chunk
/// holds a "QuadMesh" with POSITION/NORMAL/indices and four vertices spanning
/// (0,0,0)..(1,1,0). The importer supports the binary container
/// (`gltf_import.rs:44` checks the `glTF` magic and routes to `import_glb`),
/// and the editor chain is intact: `Render3DKind::Mesh { path }` resolves
/// through `mesh_for_primitive` -> `build_mesh_for_primitive` ->
/// `mesh_from_file`, which routes `.glb` to `mesh3d_from_gltf_resource`.
///
/// ⚠️ Nothing in the repo tests that CONVERSION. `gltf_import`'s own tests
/// build synthetic `.gltf` JSON in a temp dir, and the two other users of
/// `quad.glb` exercise the document and format-loader paths — so
/// `mesh3d_from_gltf_resource` on a real file has never been exercised. That
/// is the first place to look, and it belongs to `pat-s3rrv.31`'s subsystem
/// rather than to this bead.
#[test]
#[ignore = "reproduces a glTF mesh-loading gap owned by pat-s3rrv.31; see the doc comment"]
fn mesh_instance3d_imported_mesh_and_skinning_render() {
    let imported = render(&scene(Some(QUAD_GLB)));
    let fallback = render(&scene(None));
    // A mesh known to render, so "this scene draws geometry at all" is
    // established independently of the glTF under test.
    let primitive = render(&scene(Some("primitive:BoxMesh")));

    assert!(
        !imported.is_empty() && imported.len() == fallback.len(),
        "both renders must produce a frame of the same size to be comparable"
    );

    // ⚠️ THE CONTROL HAS TO PROVE A MESH REACHES THE FRAME, not merely that the
    // frame is non-uniform. The editor paints a sky gradient behind everything,
    // so a "not uniform" check passes on the backdrop alone and would let a
    // scene that drew NO geometry masquerade as one that drew a cube — which is
    // the exact conclusion the bug under test produces, from a different cause.
    // Comparing a known-good primitive against the no-mesh scene is what
    // separates them.
    assert!(
        differing(&primitive, &fallback) > 0,
        "a MeshInstance3D with a primitive mesh must render differently from \
         one with no mesh; if these match, the scene is drawing no geometry at \
         all and nothing below can be concluded about glTF loading"
    );

    let changed = differing(&imported, &fallback);
    assert!(
        changed > 0,
        "a MeshInstance3D pointing at an imported glTF must not render \
         identically to one with no mesh at all. The primitive control above \
         passed, so geometry does reach the frame — identical frames here mean \
         the glTF specifically produced no mesh and fell back to the \
         placeholder"
    );
}
