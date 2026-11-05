//! pat-y2ylj — a seeded editor Play of Dodge the Creeps spawns its mobs where
//! real Godot does.
//!
//! games/dodge_the_creeps (the official Godot demo, unmodified) spawns each
//! mob in `main.gd::_on_MobTimer_timeout`:
//!
//! ```gdscript
//! mob_spawn_location.progress_ratio = randf()
//! mob.position = mob_spawn_location.position
//! ```
//!
//! Three things had to agree with Godot for those points to match a Godot
//! reference capture pinned to the same seed (scripts/godot-reference.mjs,
//! `seed(42)` before anything runs):
//! 1. the generator — Patina's is now a bit-exact port of Godot's PCG32 and
//!    `Math::randf`;
//! 2. the draws Godot makes while CONSTRUCTING the scene — the Player's `Trail`
//!    GPUParticles2D takes two (`particles_create` + `set_seed(Math::rand())`),
//!    so the first mob's `randf()` is the THIRD number of the stream;
//! 3. `progress_ratio`'s setter moves the PathFollow2D AT ONCE, as Godot's
//!    `set_progress` -> `_update_transform()` does; Patina moved it on the next
//!    frame, so every mob spawned at the PREVIOUS spawn's point.
//!
//! The expected points are Godot 4.6.1's own: a pinned capture of the game's
//! playtest scenario (`node scripts/godot-reference.mjs games/dodge_the_creeps
//! --watch /root/Main/MobPath/MobSpawnLocation:position`) records the spawn
//! location at f152, f182 and f212 — the first three MobTimer timeouts.

use gdeditor::editor_server::{build_play_tree, EditorState};
use gdscene::packed_scene::PackedScene;
use gdscene::{NodeId, SceneTree};
use gdvariant::Variant;

/// Godot's first three spawn points for seed 42 (the capture's trace).
const GODOT_SPAWNS: [(f64, f64); 3] = [
    (0.0, 409.581939697266),
    (480.0, 111.688621520996),
    (472.560913085938, 720.0),
];
/// Godot's `progress_ratio` at those spawns.
const GODOT_RATIOS: [f64; 3] = [0.829341351985931, 0.246538206934929, 0.503103733062744];

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/dodge_the_creeps")
        .canonicalize()
        .expect("games/dodge_the_creeps fixture")
}

fn vec2(v: Variant) -> (f64, f64) {
    match v {
        Variant::Vector2(p) => (p.x as f64, p.y as f64),
        other => panic!("expected a Vector2, got {other:?}"),
    }
}

fn float(v: Variant) -> f64 {
    match v {
        Variant::Float(f) => f,
        other => panic!("expected a Float, got {other:?}"),
    }
}

fn mobs(tree: &SceneTree, main: NodeId) -> Vec<NodeId> {
    tree.get_node(main)
        .map(|n| n.children().to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter(|&c| tree.get_node(c).is_some_and(|n| n.class_name() == "RigidBody2D"))
        .collect()
}

#[test]
fn seeded_play_spawns_the_first_three_mobs_at_godots_points() {
    gdobject::class_db::register_all_classes();
    let dir = game_dir();
    gdcore::set_path_roots(Some(dir.clone()), None);
    let text = std::fs::read_to_string(dir.join("main.tscn")).unwrap();
    let scene = PackedScene::from_tscn(&text).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // Opened for editing BEFORE the seed is pinned, as in the editor: the
    // constructor draws of this load must not count.
    gdscene::packed_scene::add_packed_scene_to_tree_for_editing(&mut tree, root, &scene)
        .expect("main.tscn opens for editing");
    let mut state = EditorState::new(tree);
    state
        .texture_cache
        .set_project_root(dir.to_string_lossy().to_string());
    state.scene_file = Some("main.tscn".to_string());

    // What the Play handler does for `{"seed": 42}`.
    gdscript_interop::interpreter::pin_random_seed(Some(42));
    let mut played = build_play_tree(&mut state, None);

    let main = played.get_node_by_path("/root/Main").expect("Main");
    let follow = played
        .get_node_by_path("/root/Main/MobPath/MobSpawnLocation")
        .expect("MobSpawnLocation");
    for (i, (&(gx, gy), &ratio)) in GODOT_SPAWNS.iter().zip(&GODOT_RATIOS).enumerate() {
        let before = mobs(&played, main);
        played.call_script_method(main, "_on_MobTimer_timeout", &[]);
        let after = mobs(&played, main);
        let spawned: Vec<NodeId> = after.into_iter().filter(|m| !before.contains(m)).collect();
        assert_eq!(spawned.len(), 1, "spawn #{i}: one mob per timeout");

        let r = float(played.get_node(follow).unwrap().get_property("progress_ratio"));
        assert!(
            (r - ratio).abs() < 1e-6,
            "spawn #{i}: progress_ratio = randf() must be Godot's {ratio}, got {r} \
             (a different number = the stream, or the constructor draws before it, differ)"
        );
        let (x, y) = vec2(played.get_node(spawned[0]).unwrap().get_property("position"));
        assert!(
            (x - gx).abs() < 1e-3 && (y - gy).abs() < 1e-3,
            "spawn #{i}: the mob must start where Godot's did, ({gx}, {gy}), got ({x}, {y}) — \
             a mob at the previous spawn's point means the PathFollow2D moved a frame late"
        );
    }
    gdscript_interop::interpreter::pin_random_seed(None);
}
