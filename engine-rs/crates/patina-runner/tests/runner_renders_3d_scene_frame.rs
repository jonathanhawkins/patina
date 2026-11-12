//! pat-s3rrv.32 acceptance: the runner produces real 3D frames.
//!
//! `patina-runner` described itself as a headless runner that "dumps the final
//! tree state as JSON to stdout", and that was the whole story: `grep gdrender3d`
//! across the crate returned nothing and its `Cargo.toml` named no render crate.
//! A project run through the runner produced no pixels at all. patina-web was in
//! the same position — its entire render surface was `present_clear(r, g, b)`.
//!
//! The renderer was never missing, only unreachable: `RenderServer3DAdapter`
//! already walked a `SceneTree` and drove `SoftwareRenderer3D`, and had **no
//! production callers**. This drives the newly wired entry point in-process.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use patina_runner::render3d::render_scene_frame;

const W: u32 = 96;
const H: u32 = 72;

fn tree_from(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

/// A lit cube square in front of the camera.
///
/// `primitive:BoxMesh` rather than hand-authored geometry on purpose: it comes
/// from the repo's own generator, whose winding is already proven to survive
/// back-face culling by `solid_cube_produces_filled_pixels`. A fixture that
/// renders nothing would make "the runner cannot render" and "the model is
/// invisible" indistinguishable.
const MESH_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true

[node name="Sun" type="DirectionalLight3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 4, 0)

[node name="Cube" type="MeshInstance3D" parent="."]
mesh = "primitive:BoxMesh"
"#;

/// The same camera with nothing to look at.
const EMPTY_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true
"#;

/// **The acceptance test.**
#[test]
fn runner_renders_3d_scene_frame() {
    gdobject::class_db::register_3d_classes();

    let empty = render_scene_frame(&tree_from(EMPTY_SCENE), W, H);
    assert_eq!(
        empty.pixels.len(),
        (W * H) as usize,
        "the runner must produce a full frame buffer, not an empty vec"
    );

    // The background is taken from the empty frame itself rather than assumed.
    // The renderer works in linear HDR and tonemaps on the way out, so the
    // cleared pixel is not necessarily the literal clear colour — hard-coding
    // one would test my guess about the post-process, not the render.
    let background = empty.pixels[0];

    // An empty scene is a UNIFORM background frame — the acceptance's second
    // half, and the fixture guard for the first: if this frame were non-uniform,
    // the coverage comparison below would be meaningless.
    assert_eq!(
        empty.coverage(background),
        0,
        "a camera-only scene must render a uniform background frame"
    );

    // …and the lit cube covers pixels. Before this bead the runner had no
    // render path at all, so there were no pixels to count.
    let meshed = render_scene_frame(&tree_from(MESH_SCENE), W, H);
    let covered = meshed.coverage(background);
    assert!(
        covered > 0,
        "a lit MeshInstance3D must project non-background pixels through the \
         runner's render entry point; got {covered}. Zero means the runner still \
         has no scene submission — or that the cube is not in view."
    );
}
