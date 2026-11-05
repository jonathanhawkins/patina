//! pat-ujiku: a `.glb` mesh is imported once per FILE, not once per mesh node
//! per frame, and a `res://` glb path resolves through the project root (and
//! mounted packs) instead of being handed to `std::fs` verbatim.
//!
//! Before the fix `RenderServer3DAdapter::resolve_mesh` (and the editor's
//! `mesh_from_file`) called `gdresource::import_gltf(Path::new(path))` for every
//! node on every frame. With the path written as `res://player/player.glb` —
//! which is how a pack-loaded scene names it — `std::fs` found no such file and
//! the node silently drew the placeholder cube.
//!
//! Drives the unmodified official platformer_3d models.

use gdscene::node::Node;
use gdscene::render_server_3d::{gltf_import_count, RenderServer3DAdapter};
use gdscene::{NodeId, SceneTree};
use gdvariant::Variant;
use std::path::PathBuf;

/// Serializes the tests in this binary that touch the res:// path roots and the process-wide glTF import counter (pat-hukbz).
///
/// That state is process-global. Nextest gives each test its own process, but
/// plain `cargo test` runs the tests as threads in ONE process, where they
/// overwrote each other's state mid-test. Bind the guard for the whole test.
/// Poison-tolerant, so one failing test does not fail the rest.
#[must_use = "bind it for the whole test: `let _guard = serial();`"]
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn platformer_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../games/platformer_3d")
}

/// A tree holding `count` MeshInstance3D nodes that each draw mesh 0 of `glb`,
/// named the way a pack-loaded scene names it: by its `res://` path.
fn tree_with_glb_nodes(glb: &str, count: usize) -> (SceneTree, Vec<NodeId>) {
    gdobject::class_db::register_all_classes();
    gdcore::file_access::set_path_roots(Some(platformer_root()), None);
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ids = (0..count)
        .map(|i| {
            let mut node = Node::new(&format!("Mesh{i}"), "MeshInstance3D");
            node.set_property("mesh", Variant::String(glb.into()));
            node.set_property("gltf_mesh_index", Variant::Int(0));
            tree.add_child(root, node).unwrap()
        })
        .collect();
    (tree, ids)
}

#[test]
fn a_res_path_glb_mesh_resolves_through_the_project_root() {
    let _guard = serial();
    // The test process runs from crates/gdscene, so the bare `res://` string is
    // not a file `std::fs` can open from here.
    assert!(!std::path::Path::new("res://player/player.glb").exists());
    let (tree, ids) = tree_with_glb_nodes("res://player/player.glb", 1);
    let mesh = RenderServer3DAdapter::resolve_mesh(&tree, ids[0]);
    // The placeholder is `Mesh3D::cube(1.0)`: 24 vertices.
    assert!(
        mesh.vertices.len() > 100,
        "player.glb resolved to {} vertices — the placeholder cube, not the robot",
        mesh.vertices.len()
    );
}

#[test]
fn a_glb_is_parsed_once_however_many_nodes_and_frames_draw_it() {
    let _guard = serial();
    // enemy.glb is used by no other test in this binary, so the import
    // counter's delta belongs to this test alone.
    let (tree, ids) = tree_with_glb_nodes("res://enemy/enemy.glb", 4);
    let before = gltf_import_count();
    let mut sizes = Vec::new();
    for _frame in 0..5 {
        for id in &ids {
            sizes.push(RenderServer3DAdapter::resolve_mesh(&tree, *id).vertices.len());
        }
    }
    assert!(sizes.iter().all(|n| *n > 24), "enemy.glb must resolve, got {sizes:?}");
    assert!(sizes.windows(2).all(|w| w[0] == w[1]), "every call draws the same mesh");
    assert_eq!(
        gltf_import_count() - before,
        1,
        "20 resolutions of one file must parse it once"
    );
}
