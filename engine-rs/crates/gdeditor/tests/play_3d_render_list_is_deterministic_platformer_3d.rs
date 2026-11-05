//! pat-414k3: the 3D render list lists a GridMap's cells in the SAME order on
//! every frame.
//!
//! `GridMap::from_node` rebuilds the cell map afresh on every call, and a
//! `HashMap` gets a fresh random seed each time, so the editor and Play views
//! listed the official 3D platformer's 3000+ cells in a different order on every
//! frame. Draw order decides which of two coplanar surfaces wins the depth
//! test, so a PAUSED game's frame flickered — and the renderer's output could
//! not be compared with itself, let alone before and after an optimisation.
//!
//! Drives the unmodified `games/platformer_3d/stage/stage.tscn`.

use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::Render3DKind;
use gdscene::SceneTree;

fn load_stage() -> SceneTree {
    gdobject::class_db::register_all_classes();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../games/platformer_3d");
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let tscn = std::fs::read_to_string(dir.join("stage/stage.tscn")).expect("stage.tscn");
    let packed = gdscene::packed_scene::PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree_from_res(&mut tree, root, &packed).unwrap();
    tree
}

#[test]
fn the_grid_map_cells_come_out_in_the_same_order_every_frame() {
    let tree = load_stage();
    let vp = Viewport3D::default();
    let first = vp.build_render_list(&tree);
    let tiles = first
        .iter()
        .filter(|i| matches!(&i.kind, Render3DKind::Mesh { path } if path.starts_with("res://stage/meshes/")))
        .count();
    assert!(tiles > 2000, "only {tiles} GridMap tiles in the list — the level did not load");
    for frame in 1..4 {
        let again = vp.build_render_list(&tree);
        let moved = first
            .iter()
            .zip(&again)
            .filter(|(a, b)| a.global_transform != b.global_transform || a.kind != b.kind)
            .count();
        assert_eq!(first.len(), again.len());
        assert_eq!(moved, 0, "frame {frame}: {moved} items changed place in the list");
    }
}
