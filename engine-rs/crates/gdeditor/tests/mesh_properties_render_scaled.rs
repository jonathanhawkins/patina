//! pat-0q2kx.43 acceptance: a mesh renders at its **authored** dimensions, and a
//! `.obj` ext-resource renders that file's geometry rather than a stand-in cube.
//!
//! `build_mesh_for_primitive` returned fixed-size primitives — `BoxMesh` was always
//! a 0.8 cube, `PrismMesh` was a cone, and anything file-backed fell through to a
//! default cube — while `gdserver3d` had real generators taking real parameters
//! (`BoxMesh { size }`, `SphereMesh { radius, height, … }`, `PrismMesh`) and
//! `Mesh3D::from_obj` sat unused. So a 20×1×20 floor rendered as a small cube.
//!
//! Silhouettes are measured by **differencing against a render of the same scene
//! with no mesh**, which isolates the mesh's pixels exactly and is immune to the
//! viewport's sky gradient (a fixed "background colour" test would not be).

use gdcore::math::Vector3;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::render_editor_3d_frame;
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::{Node, NodeId};
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const W: u32 = 128;
const H: u32 = 96;

/// The render list filters on `is_class("Node3D")`, which reads the ClassDB —
/// without registration nothing is collected and every assertion below would pass
/// vacuously.
fn new_tree() -> (SceneTree, NodeId) {
    gdobject::class_db::register_3d_classes();
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();
    (tree, root)
}

fn render(tree: &SceneTree) -> FrameBuffer {
    render_editor_3d_frame(&Viewport3D::new(W, H), tree)
}

/// `(width, height)` of the bounding box of pixels that differ between the two
/// frames — i.e. the mesh's silhouette. `(0, 0)` when nothing was drawn.
fn silhouette(with_mesh: &FrameBuffer, empty: &FrameBuffer) -> (u32, u32) {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
    let mut any = false;
    for y in 0..with_mesh.height {
        for x in 0..with_mesh.width {
            let i = (y * with_mesh.width + x) as usize;
            let (a, b) = (with_mesh.pixels[i], empty.pixels[i]);
            if (a.r - b.r).abs() > 0.01 || (a.g - b.g).abs() > 0.01 || (a.b - b.b).abs() > 0.01 {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if !any {
        return (0, 0);
    }
    (max_x - min_x + 1, max_y - min_y + 1)
}

/// Renders a scene whose single `MeshInstance3D` uses `mesh_path`, with optional
/// `__mesh__*` companions carrying the sub-resource's authored properties.
fn render_mesh(mesh_path: &str, props: &[(&str, Variant)]) -> FrameBuffer {
    let (mut tree, root) = new_tree();
    let mut node = Node::new("Mesh", "MeshInstance3D");
    node.set_property("mesh", Variant::String(mesh_path.to_string()));
    let id = tree.add_child(root, node).unwrap();
    node3d::set_position(&mut tree, id, Vector3::ZERO);
    for (key, value) in props {
        tree.get_node_mut(id)
            .unwrap()
            .set_property(key, value.clone());
    }
    render(&tree)
}

fn render_empty() -> FrameBuffer {
    let (tree, _root) = new_tree();
    render(&tree)
}

/// **The acceptance test.** `BoxMesh.size` reaches the geometry: flattening Y
/// shortens the silhouette while leaving its width alone, which a fixed-size cube
/// cannot do.
#[test]
fn mesh_properties_render_scaled() {
    let empty = render_empty();

    let unit = silhouette(&render_mesh("primitive:BoxMesh", &[]), &empty);
    let flat = silhouette(
        &render_mesh(
            "primitive:BoxMesh",
            &[(
                "__mesh__size",
                Variant::Vector3(Vector3::new(1.0, 0.2, 1.0)),
            )],
        ),
        &empty,
    );

    assert!(
        unit.0 > 0 && unit.1 > 0,
        "the baseline box must render at all, got {unit:?}",
    );
    assert!(
        flat.0 > 0 && flat.1 > 0,
        "the flattened box must render at all, got {flat:?}",
    );
    // Only Y was authored down, so the silhouette gets shorter…
    assert!(
        (flat.1 as f32) < (unit.1 as f32) * 0.8,
        "an authored size of (1, 0.2, 1) must render shorter than the default \
         (1, 1, 1): flattened {flat:?} vs default {unit:?}",
    );
    // …while its width is essentially unchanged, which pins that the authored
    // property reached the generator rather than the whole mesh being rescaled.
    let width_ratio = flat.0 as f32 / unit.0 as f32;
    assert!(
        (0.8..=1.25).contains(&width_ratio),
        "flattening Y must not change the footprint width: {flat:?} vs {unit:?}",
    );

    // A wider box also grows its footprint, the other half of honouring `size`.
    let wide = silhouette(
        &render_mesh(
            "primitive:BoxMesh",
            &[(
                "__mesh__size",
                Variant::Vector3(Vector3::new(2.0, 1.0, 2.0)),
            )],
        ),
        &empty,
    );
    assert!(
        wide.0 > unit.0,
        "a 2-unit box must cover more width than a 1-unit box: {wide:?} vs {unit:?}",
    );
}

/// The **positional** marker form — `primitive:BoxMesh|1,0.2,1` — decodes to the
/// same geometry as the named form.
///
/// This is the encoding a `.tscn` load will produce, mirroring
/// `packed_scene::encode_shape_marker`'s `shape:Type|d0,d1,d2`. It is pinned here
/// separately because the two halves ship independently: the renderer (this crate)
/// decodes, and `packed_scene.rs` encodes. Without this test a mismatch between
/// them would be invisible — the marker would fall through to a default-sized box
/// while every other assertion in this file still passed.
#[test]
fn positional_marker_form_decodes_like_the_named_form() {
    let empty = render_empty();

    let unit = silhouette(&render_mesh("primitive:BoxMesh", &[]), &empty);
    // Same flattening as the acceptance test, expressed the way a .tscn will.
    let positional = silhouette(&render_mesh("primitive:BoxMesh|1,0.2,1", &[]), &empty);
    let named = silhouette(
        &render_mesh(
            "primitive:BoxMesh",
            &[(
                "__mesh__size",
                Variant::Vector3(Vector3::new(1.0, 0.2, 1.0)),
            )],
        ),
        &empty,
    );

    assert!(
        positional.0 > 0 && positional.1 > 0,
        "the positional marker must still render a mesh, got {positional:?}",
    );
    assert_eq!(
        positional, named,
        "`primitive:BoxMesh|1,0.2,1` must decode to exactly the geometry the \
         `__mesh__size` companion produces — the encoder and decoder ship in \
         different crates and this is the contract between them",
    );
    assert!(
        (positional.1 as f32) < (unit.1 as f32) * 0.8,
        "the positional form must actually apply the authored size: {positional:?} \
         vs default {unit:?}",
    );
}

/// A `MeshInstance3D` pointing at an `.obj` file renders that file's geometry, not
/// the placeholder cube.
#[test]
fn obj_ext_resource_renders_its_own_geometry() {
    // Unique per process: a fixed temp path races when several test binaries run
    // at once.
    let path = std::env::temp_dir().join(format!("patina_mesh_{}.obj", std::process::id()));
    // A tall, narrow triangle — nothing like a cube in silhouette.
    std::fs::write(
        &path,
        "v 0.0 1.5 0.0\nv -0.15 -1.5 0.0\nv 0.15 -1.5 0.0\nf 1 2 3\n",
    )
    .expect("write the obj fixture");
    let path_str = path.to_string_lossy().to_string();

    let empty = render_empty();
    let cube = silhouette(&render_mesh("primitive:BoxMesh", &[]), &empty);
    let obj = silhouette(&render_mesh(&path_str, &[]), &empty);
    let _ = std::fs::remove_file(&path);

    assert!(
        obj.0 > 0 && obj.1 > 0,
        "the .obj mesh must render something, got {obj:?}",
    );
    // The fixture is ~3 units tall and ~0.3 wide, so it must read as much taller
    // than wide — the placeholder cube reads roughly square.
    let obj_aspect = obj.1 as f32 / obj.0 as f32;
    let cube_aspect = cube.1 as f32 / cube.0 as f32;
    assert!(
        obj_aspect > cube_aspect * 1.5,
        "the .obj triangle must render its own tall silhouette rather than a cube: \
         obj {obj:?} (aspect {obj_aspect:.2}) vs cube {cube:?} (aspect {cube_aspect:.2})",
    );
}
