//! The official 3D platformer keeps its whole level in a binary,
//! RSCC-compressed `stage/grid_map.scn` instanced from `stage/stage.tscn`, and
//! every tile mesh / collision shape is a binary `.res` (pat-33wmr).
//!
//! Before the fix the instanced GridMap came up as an empty `Node` (the
//! sub-scene was read with `read_to_string`, which fails on compressed bytes),
//! and an `ArrayMesh` file anywhere fell through to the placeholder cube. These
//! tests drive the unmodified game files through the runtime load path.

use gdscene::{GridMap, MeshLibrary, SceneTree};
use gdvariant::Variant;

fn project() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../games/platformer_3d")
}

/// `stage.tscn` loaded the way a running game loads it (sub-scenes expanded).
fn load_stage() -> SceneTree {
    gdobject::class_db::register_all_classes();
    let dir = project();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let tscn = std::fs::read_to_string(dir.join("stage/stage.tscn")).expect("stage.tscn");
    let packed = gdscene::packed_scene::PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree_from_res(&mut tree, root, &packed).unwrap();
    tree
}

#[test]
fn the_instanced_binary_grid_map_is_a_grid_map_with_its_library_and_cells() {
    let tree = load_stage();
    let grid_id = tree
        .get_node_by_path("/root/Stage/GridMap")
        .expect("GridMap node");
    let node = tree.get_node(grid_id).unwrap();
    assert_eq!(
        node.class_name(),
        "GridMap",
        "the binary .scn root must instance as its real class, not a bare Node"
    );
    assert_eq!(
        node.get_property("__extres__mesh_library"),
        Variant::String("res://stage/tiles.tres".into()),
        "the .scn's MeshLibrary ExtResource must resolve"
    );
    // cell_octant_size is authored (16, default 8) in the .scn only, never in
    // stage.tscn, so it proves the binary scene's own properties arrived.
    assert_eq!(node.get_property("cell_octant_size"), Variant::Int(16));

    let mut grid = GridMap::from_node(&tree, grid_id).expect("GridMap::from_node");
    assert!(grid.cell_count() > 2000, "only {} cells", grid.cell_count());
    grid.mesh_library = MeshLibrary::load("res://stage/tiles.tres").expect("tiles.tres");
    assert!(
        grid.mesh_library.item_count() >= 15,
        "{} items",
        grid.mesh_library.item_count()
    );

    // Every placed cell resolves to a tile mesh that DECODES — none falls
    // back to the placeholder cube.
    let instances = grid.mesh_instances_oriented();
    assert_eq!(
        instances.len(),
        grid.cell_count(),
        "every cell names a library item"
    );
    let mut paths: Vec<String> = instances.into_iter().map(|(_, _, p)| p).collect();
    paths.sort();
    paths.dedup();
    for path in &paths {
        assert!(path.ends_with(".res"), "{path}");
        let mesh = gdscene::array_mesh::load_array_mesh_file(path)
            .unwrap_or_else(|| panic!("{path} must decode"));
        assert!(!mesh.vertices.is_empty(), "{path}");
    }
}

#[test]
fn a_mesh_instance_with_a_binary_array_mesh_resolves_to_that_geometry() {
    let tree = load_stage();
    let floor = tree
        .get_node_by_path("/root/Stage/ExtraFloor1")
        .expect("ExtraFloor1");
    assert_eq!(
        gdscene::node3d::get_mesh_path(&tree, floor).as_deref(),
        Some("res://stage/meshes/floor.res"),
        "`mesh = ExtResource(..)` must resolve to the file, not the reference literal"
    );
    let mesh = gdscene::render_server_3d::RenderServer3DAdapter::resolve_mesh(&tree, floor);
    // floor.res is a 4-vertex quad; the placeholder cube has 24 vertices.
    assert_eq!(mesh.vertices.len(), 4, "got the placeholder, not floor.res");
    assert!(mesh.vertices.iter().all(|v| v.y == -1.0));
}

#[test]
fn a_binary_scene_file_loads_as_the_main_scene() {
    gdobject::class_db::register_all_classes();
    let dir = project();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = gdscene::res_boot::load_scene_into_tree(&mut tree, root, "res://stage/grid_map.scn")
        .expect("a .scn loads as a scene");
    assert_eq!(tree.get_node(id).unwrap().class_name(), "GridMap");
    assert!(GridMap::from_node(&tree, id).unwrap().cell_count() > 2000);
}
