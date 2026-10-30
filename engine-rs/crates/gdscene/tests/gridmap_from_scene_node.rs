//! pat-s3rrv.89 (increment) — a `GridMap` can be read from a **live scene node**.
//!
//! `GridMap::from_tscn` parses `.tscn` *source* and resolves the mesh library
//! through the `PackedScene`. By render time that scene is gone and only the
//! `SceneTree` remains, so nothing on the runtime side could build a `GridMap`
//! at all — which is why the editor/Play render list has never emitted one.
//! `from_node` is the missing reader that the render wiring needs.
//!
//! Godot ground truth: `upstream/godot/modules/gridmap/grid_map.cpp` —
//! `cell_size`, `mesh_library`, and the `data/cells` packed array.

use gdscene::grid_map::{GridCell, GridMap, MeshLibrary};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::{Dictionary, Variant};

use gdcore::math::Vector3;

/// `data/cells` triples are `(key_lo, key_hi, packed_value)`; `key_lo`'s low 16
/// bits are x, so `2` is cell (2,0,0), and the value's low 16 bits are the mesh
/// library item id.
const CELLS: [i32; 3] = [2, 0, 1];

fn grid_node(tree: &mut SceneTree, mesh_library: Option<(&str, &str)>) -> gdscene::NodeId {
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("Grid", "GridMap")).unwrap();
    let node = tree.get_node_mut(id).unwrap();
    node.set_property("cell_size", Variant::Vector3(Vector3::new(4.0, 4.0, 4.0)));
    let mut data = Dictionary::new();
    data.insert("cells", Variant::PackedInt32Array(CELLS.to_vec()));
    node.set_property("data", Variant::Dictionary(data.into()));
    if let Some((key, value)) = mesh_library {
        node.set_property(key, Variant::String(value.to_string()));
    }
    id
}

#[test]
fn gridmap_from_scene_node() {
    let mut tree = SceneTree::new();
    // `packed_scene` parks an ExtResource's resolved path in the
    // `__extres__<prop>` companion, which is what an instanced node carries.
    let id = grid_node(
        &mut tree,
        Some(("__extres__mesh_library", "res://lib.tres")),
    );

    let grid = GridMap::from_node(&tree, id).expect("a GridMap node must load");

    assert_eq!(
        grid.cell_size, 4.0,
        "cell_size comes from the node's Vector3"
    );
    assert_eq!(
        grid.mesh_library_resource.as_deref(),
        Some("res://lib.tres"),
        "the resolved ExtResource path must be preferred"
    );

    let used = grid.used_cells();
    assert_eq!(used.len(), 1, "exactly the one painted cell, got {used:?}");
    let cell = GridCell { x: 2, y: 0, z: 0 };
    assert_eq!(used[0], cell, "the packed key decodes to cell (2,0,0)");
    assert_eq!(
        grid.get_cell_item(cell),
        1,
        "the packed value's low bits are the mesh-library item id"
    );
}

/// Control: a node of any other class is not a GridMap, so the reader must
/// decline rather than hand back an empty grid that reads like a cleared map.
#[test]
fn a_non_gridmap_node_is_declined() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree
        .add_child(root, Node::new("NotAGrid", "Node3D"))
        .unwrap();
    assert!(
        GridMap::from_node(&tree, id).is_none(),
        "from_node must return None for a non-GridMap node"
    );
}

/// Control: an UNRESOLVED `ExtResource("…")` reference is not a path. Storing
/// it as one would make the render path try to load a file called
/// `ExtResource("1_lib")`.
#[test]
fn an_unresolved_ext_resource_is_not_stored_as_a_path() {
    let mut tree = SceneTree::new();
    let id = grid_node(&mut tree, Some(("mesh_library", "ExtResource(\"1_lib\")")));

    let grid = GridMap::from_node(&tree, id).expect("a GridMap node must load");
    assert_eq!(
        grid.mesh_library_resource, None,
        "an unresolved reference must not be mistaken for a resource path"
    );
    // The cells still load — a missing library must not lose the map.
    assert_eq!(grid.used_cells().len(), 1, "cells load regardless");
}

/// A Godot `MeshLibrary` `.tres`: one usable item, plus an item that carries a
/// name but no mesh.
const MESH_LIBRARY_TRES: &str = r#"[gd_resource type="MeshLibrary" load_steps=2 format=3]

[ext_resource type="Mesh" path="res://meshes/cube.obj" id="1_cube"]

[resource]
item/0/name = "placeholder"
item/1/name = "cube"
item/1/mesh = ExtResource("1_cube")
item/1/mesh_transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)
"#;

/// A GridMap node references its library by PATH and nothing ever turned that
/// path into items, so `mesh_instances()` was empty for every cell.
#[test]
fn mesh_library_loads_from_tres() {
    let lib = MeshLibrary::from_tres(MESH_LIBRARY_TRES)
        .expect("the .tres parses")
        .expect("a MeshLibrary resource must load");

    // Item 1, not 0: the cell's item id is 1, and keying the usable item off a
    // NON-zero id is what proves the lookup uses the real id rather than
    // defaulting to the first entry.
    let item = lib.get_item(1).expect("item 1 is present");
    assert_eq!(item.name, "cube");
    assert_eq!(
        item.mesh_path, "res://meshes/cube.obj",
        "the ExtResource reference must resolve to its path"
    );

    assert_eq!(
        lib.item_count(),
        1,
        "an item with a name but no mesh contributes no geometry and must be \
         skipped rather than admitted with an empty path"
    );
    assert!(
        lib.get_item(0).is_none(),
        "the mesh-less item is not an item"
    );
}

/// Control: a resource of another class must be declined, not read as an empty
/// library — which would look like a library with no items.
#[test]
fn a_non_mesh_library_resource_is_declined() {
    let other = r#"[gd_resource type="Theme" format=3]

[resource]
item/0/name = "not a mesh library"
"#;
    assert!(
        MeshLibrary::from_tres(other).expect("parses").is_none(),
        "from_tres must decline a non-MeshLibrary resource"
    );
}

/// The two halves together: a node's cells resolved through a loaded library
/// give one mesh per used cell **at the right world position** — the acceptance
/// clause this increment exists to make reachable.
#[test]
fn used_cells_resolve_to_meshes_at_world_positions() {
    let mut tree = SceneTree::new();
    let id = grid_node(
        &mut tree,
        Some(("__extres__mesh_library", "res://lib.tres")),
    );
    let mut grid = GridMap::from_node(&tree, id).expect("a GridMap node must load");

    // Before the library is loaded there is nothing to resolve to — which is
    // exactly the state the render path has always been in.
    assert!(
        grid.mesh_instances().is_empty(),
        "a cell cannot resolve to a mesh until the library is loaded"
    );

    grid.mesh_library = MeshLibrary::from_tres(MESH_LIBRARY_TRES)
        .expect("the .tres parses")
        .expect("a MeshLibrary resource must load");

    let instances = grid.mesh_instances();
    assert_eq!(instances.len(), 1, "one instance per used cell");
    let (position, path) = &instances[0];
    // cell (2,0,0) at cell_size 4.0.
    assert_eq!(
        *position,
        Vector3::new(8.0, 0.0, 0.0),
        "the instance sits at cell * cell_size"
    );
    assert_eq!(path, "res://meshes/cube.obj");
}
