//! pat-nzgdu.85 — `SceneTree::change_scene_to_file` loads a `.tscn` from a
//! `res://` path and swaps the running scene at the next frame boundary
//! (Godot's deferred timing), and `quit()` raises the runner's stop flag.
//!
//! Godot ground truth: `get_tree().change_scene_to_file("res://b.tscn")` frees
//! the current scene and instances `b.tscn` as the new `current_scene`, but
//! *deferred* — the swap happens at the end of the frame so the calling
//! script's own node is not freed out from under it. `SceneTree.quit()` sets a
//! flag the main loop polls to stop.
//!
//! This test drives the path through `MainLoop::step` (which applies the
//! deferred change), asserting the scene is still the old one until the step
//! runs and is the new one afterward.

use gdscene::packed_scene::PackedScene;
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;

/// A unique temp directory for this test's `res://` root.
fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("patina_change_scene_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn change_scene_to_file_swaps_running_scene() {
    // A `res://` root containing the scene we will switch to.
    let dir = temp_dir();
    std::fs::write(
        dir.join("b.tscn"),
        "[gd_scene format=3]\n\n[node name=\"SceneB\" type=\"Node2D\"]\n",
    )
    .unwrap();
    gdcore::set_path_roots(Some(dir.clone()), None);

    // Start running on scene A.
    let mut tree = SceneTree::new();
    let scene_a =
        PackedScene::from_tscn("[gd_scene format=3]\n\n[node name=\"SceneA\" type=\"Node2D\"]\n")
            .unwrap();
    tree.change_scene_to_packed(&scene_a).unwrap();
    let a_root = tree.current_scene().expect("scene A is current");
    assert_eq!(tree.get_node(a_root).unwrap().name(), "SceneA");

    let mut ml = MainLoop::new(tree);

    // change_scene_to_file is DEFERRED: it queues the (already-parsed) scene but
    // does not swap yet — current_scene is still A.
    ml.tree_mut()
        .change_scene_to_file("res://b.tscn")
        .expect("change_scene_to_file loads and queues b.tscn");
    assert!(
        ml.tree().has_pending_scene_change(),
        "the swap is deferred to the frame boundary, not applied immediately"
    );
    let still_a = ml.tree().current_scene().unwrap();
    assert_eq!(
        ml.tree().get_node(still_a).unwrap().name(),
        "SceneA",
        "current scene is unchanged until the next frame"
    );

    // Stepping one frame applies the deferred change.
    ml.step(1.0 / 60.0);

    let b_root = ml
        .tree()
        .current_scene()
        .expect("scene B is current after one frame");
    assert_eq!(
        ml.tree().get_node(b_root).unwrap().name(),
        "SceneB",
        "change_scene_to_file swapped the running scene to B by the next frame"
    );
    assert!(
        !ml.tree().has_pending_scene_change(),
        "the pending scene change was consumed"
    );

    // quit() raises the stop flag the runner polls.
    assert!(
        !ml.tree().should_quit(),
        "not quitting before quit() is called"
    );
    ml.tree_mut().quit(0);
    assert!(
        ml.tree().should_quit(),
        "quit() makes should_quit() report true so the run loop stops"
    );
    assert_eq!(ml.tree().exit_code(), 0);
}
