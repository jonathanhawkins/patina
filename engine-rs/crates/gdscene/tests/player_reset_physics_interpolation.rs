//! pat-ghw2s — `Node.reset_physics_interpolation()` from a script, with Godot's
//! semantics, on the official 3D platformer's UNMODIFIED `player.tscn` and
//! `player.gd`.
//!
//! `player.gd` respawns the player when it falls below y = -12:
//!
//! ```gdscript
//! position = initial_position
//! velocity = Vector3.ZERO
//! reset_physics_interpolation()
//! ```
//!
//! and `project.godot` turns `physics/common/physics_interpolation` on. Patina
//! had the reset on `MainLoop` but no script arm, so the call raised
//! "method not found: 'reset_physics_interpolation'" on every respawn frame and
//! the rendered player streaked from the pit back to spawn. Godot resets the
//! node AND its descendants (`propagate_notification`), so the rendered
//! position is the post-teleport one immediately.

use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::PackedScene;
use gdscene::physics_interpolation_3d::interpolated_position_3d;
use gdscene::SceneTree;

const DT: f64 = 1.0 / 60.0;

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer_3d")
        .canonicalize()
        .expect("games/platformer_3d fixture")
}

/// Boots the real `player.tscn` the way the runner boots a scene: project
/// settings applied (so interpolation is ON, as `project.godot` asks), scripts
/// attached, one `add_child`, physics registered.
fn boot_player() -> (MainLoop, NodeId) {
    gdobject::class_db::register_all_classes();
    let dir = game_dir();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let text = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    let settings = gdcore::project_settings::ProjectSettings::from_godot_format(&text).unwrap();
    gdscene::project_runtime::seed_script_settings_for_this_thread(&settings, &dir);

    let tscn = std::fs::read_to_string(dir.join("player/player.tscn")).unwrap();
    let packed = PackedScene::from_tscn(&tscn).expect("player.tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::begin_subscene_session();
    let id = gdscene::packed_scene::instance_packed_scene_detached(
        &mut tree,
        &packed,
        &gdscene::packed_scene::load_scene_from_res,
    )
    .expect("player instances");
    gdscene::scripting::attach_subtree_scripts_from_res(&mut tree, id);
    let node = tree.take_node(id).expect("detached root");
    tree.add_child(root, node).expect("add_child");
    gdscene::packed_scene::finish_attached_scene(&mut tree, id);
    let mut ml = MainLoop::new(tree);
    gdscene::project_runtime::apply_project_settings(&mut ml, &settings);
    ml.register_physics_bodies();
    (ml, id)
}

#[test]
fn player_respawn_resets_interpolation_without_a_script_error() {
    let (mut ml, player) = boot_player();
    assert!(
        ml.physics_interpolation_enabled(),
        "project.godot sets physics/common/physics_interpolation=true"
    );
    let spawn = gdscene::node3d::get_position(ml.tree(), player);

    // Alone in the scene there is no floor: the player falls, and once a tick
    // starts below y = -12 `player.gd` teleports it back to spawn. The tick
    // before that one ended in the pit, so the interpolation holds a pit-side
    // snapshot that only the reset can discard.
    let mut last = spawn;
    let mut respawned = false;
    for _ in 0..600 {
        ml.step(DT);
        let p = gdscene::node3d::get_position(ml.tree(), player);
        if p.y > last.y + 5.0 {
            // A frame can run two ticks, so the last DRAWN frame may sit just
            // above the -12 line the respawn tick tested.
            assert!(last.y < -11.0, "fell to {last:?} before the respawn");
            respawned = true;
            break;
        }
        last = p;
    }
    assert!(respawned, "the player must fall below -12 and be respawned, last at {last:?}");

    let errors = ml.tree_mut().drain_script_errors();
    assert!(
        !errors.iter().any(|e| e.contains("reset_physics_interpolation")),
        "player.gd's call must resolve: {errors:?}"
    );

    let now = gdscene::node3d::get_position(ml.tree(), player);
    assert!(
        (now.y - spawn.y).abs() < 1.0,
        "the script must have respawned the player near {spawn:?}, it is at {now:?}"
    );
    // Godot's reset makes previous = current = the transform AT the call; the
    // same tick then applies gravity, so the drawn position may blend across
    // that one tick of motion (~0.01 here) — never back toward the pit, which
    // is where it blends from without the reset.
    let drawn = interpolated_position_3d(ml.tree(), player).expect("interpolation publishes");
    assert!(
        (drawn - now).length() < 0.05,
        "after reset_physics_interpolation() the player must be DRAWN where it is \
         ({now:?}), not blended back toward the pit; drawn at {drawn:?}"
    );
}

/// Godot's reset propagates to every descendant of the node it is called on.
#[test]
fn reset_request_covers_the_subtree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, gdscene::node::Node::new("A", "Node3D")).unwrap();
    let b = tree.add_child(a, gdscene::node::Node::new("B", "Node3D")).unwrap();
    let c = tree.add_child(b, gdscene::node::Node::new("C", "Node3D")).unwrap();
    gdscene::LifecycleManager::enter_tree(&mut tree, a);

    tree.request_physics_interpolation_reset(b);
    let mut got = tree.take_physics_interpolation_resets();
    got.sort_by_key(|id| format!("{id:?}"));
    let mut want = vec![b, c];
    want.sort_by_key(|id| format!("{id:?}"));
    assert_eq!(got, want, "B and its child C, not the parent A");
    assert!(tree.take_physics_interpolation_resets().is_empty(), "drained");
}
