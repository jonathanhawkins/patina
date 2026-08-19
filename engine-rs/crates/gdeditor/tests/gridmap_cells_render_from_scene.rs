//! pat-s3rrv.89 — a `GridMap`'s used cells reach the editor/Play render list.
//!
//! ⚠️ **Two halves had to meet, and neither was missing on its own.**
//! `gdscene::grid_map` decodes cells, orientations and a `MeshLibrary`, and
//! `GridMap::from_node` builds one from a live scene node. But `from_node`
//! captures only the library's *path* — the items live in a separate `.tres`
//! that nothing ever loaded — so every cell resolved to no mesh, and the editor
//! drew a single 0.4 marker cube for an entire level's worth of geometry.
//!
//! ⚠️ The cells are emitted with their **orientation**. `mesh_instances()` drops
//! it; `mesh_instances_oriented()` keeps it. A tiled map drawn unrotated reads as
//! badly authored art rather than as missing data, which is a much harder bug to
//! recognise from a screenshot.

use gdcore::math::Vector3;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{editor_3d_render_inputs, Render3DKind};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::{Dictionary, Variant};

/// A Godot `MeshLibrary` `.tres` with one usable item (id 1).
const MESH_LIBRARY_TRES: &str = r#"[gd_resource type="MeshLibrary" load_steps=2 format=3]

[ext_resource type="Mesh" path="res://meshes/cube.obj" id="1_cube"]

[resource]
item/1/name = "cube"
item/1/mesh = ExtResource("1_cube")
"#;

/// `data/cells` triples are `(key_lo, key_hi, packed_value)`: one cell at x = 2
/// holding mesh-library item 1, unrotated.
const CELLS: [i32; 3] = [2, 0, 1];

/// Builds a scene with one GridMap, optionally pointed at a mesh library.
fn grid_scene(library_path: Option<&str>) -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree
        .add_child(root, Node::new("Grid", "GridMap"))
        .expect("GridMap added");
    let node = tree.get_node_mut(id).expect("the GridMap node");
    node.set_property("cell_size", Variant::Vector3(Vector3::new(2.0, 2.0, 2.0)));
    let mut data = Dictionary::new();
    data.insert("cells", Variant::PackedInt32Array(CELLS.to_vec()));
    node.set_property("data", Variant::Dictionary(data));
    if let Some(path) = library_path {
        node.set_property("__extres__mesh_library", Variant::String(path.to_string()));
    }
    tree
}

fn mesh_items(tree: &SceneTree) -> Vec<(String, Vector3)> {
    editor_3d_render_inputs(&Viewport3D::default(), tree)
        .items
        .into_iter()
        .filter_map(|i| match i.kind {
            Render3DKind::Mesh { path } => Some((path, i.global_transform.origin)),
            _ => None,
        })
        .collect()
}

/// **The acceptance test for the GridMap third of this bead.**
#[test]
fn gridmap_cells_render_from_scene() {
    // ⚠️ BOTH, and `register_2d_classes` is the one that matters here: `GridMap`
    // is registered inside it (class_db.rs:11794) despite being a Node3D. Without
    // it the class is unknown, `is_class("Node3D")` answers false, and
    // `collect_scene_content` skips the node entirely — the render list comes back
    // empty and the failure reads as "the mesh library never loaded".
    gdobject::class_db::register_2d_classes();
    gdobject::class_db::register_3d_classes();
    let dir = tempfile::tempdir().expect("tempdir");
    let lib = dir.path().join("lib.tres");
    std::fs::write(&lib, MESH_LIBRARY_TRES).expect("write the mesh library");

    let tree = grid_scene(Some(&lib.to_string_lossy()));
    let cells = mesh_items(&tree);

    assert_eq!(
        cells.len(),
        1,
        "the one used cell must produce one mesh item; 0 means the library \
         never loaded and the cell resolved to nothing",
    );
    assert_eq!(
        cells[0].0, "res://meshes/cube.obj",
        "the cell must carry the mesh path its library item names",
    );
    // cell x = 2 at cell_size 2 → world x = 4.
    assert!(
        (cells[0].1.x - 4.0).abs() < 1e-4,
        "the cell must sit at cell * cell_size = 4 on x, got {}",
        cells[0].1.x,
    );
}

/// 🔑 The control: without the library, the cell resolves to nothing.
///
/// ⚠️ This is what proves the acceptance test above is measuring the library
/// LOAD rather than merely the presence of a GridMap node. Before this change
/// both cases produced the same output — a marker cube — which is exactly why
/// the gap was invisible.
#[test]
fn a_gridmap_without_its_library_emits_no_cells() {
    // ⚠️ BOTH, and `register_2d_classes` is the one that matters here: `GridMap`
    // is registered inside it (class_db.rs:11794) despite being a Node3D. Without
    // it the class is unknown, `is_class("Node3D")` answers false, and
    // `collect_scene_content` skips the node entirely — the render list comes back
    // empty and the failure reads as "the mesh library never loaded".
    gdobject::class_db::register_2d_classes();
    gdobject::class_db::register_3d_classes();
    let tree = grid_scene(None);

    assert!(
        mesh_items(&tree).is_empty(),
        "a cell whose library is unavailable has no mesh to draw",
    );
}
