//! pat-d4ghb / pat-hwhbj — the official 3D platformer's player, follow camera
//! and enemy, frame by frame, against REAL Godot 4.6.1 (Jolt).
//!
//! The Godot numbers are the frame-exact trace of
//! `node scripts/compare-to-godot.mjs games/platformer_3d --keys
//! "wait:500,KeyW:1500,Space:300,wait:2000"` (frame f = the state after f
//! main-loop iterations of 1/60 s, i.e. 2 physics ticks at the project's 120
//! ticks/s; W held for frames 31-120, Space for 121-138).
//!
//! ⚠️ One input detail differs from that tool: its Godot capture injects key
//! events DURING the first physics tick of a frame, and Godot then reports
//! `is_action_just_released` on the SECOND tick. A real key (and this test,
//! and the editor's Play) arrives between frames, where Godot reports it on the
//! first tick. The jump-release numbers below were measured with a real
//! between-frame release (a `_process` release in a probe copy of the game).
//!
//! Before these fixes the player diverged from frame 5 (max error 1.38 m):
//! `get_gravity()` was live on a body's first tick (Godot reads ZERO before
//! any step), the 3D `move_and_slide()` was deferred to the next tick, every
//! falling body within `floor_snap_length` of a floor was snapped onto it, a
//! jump release cut the jump twice (once per physics tick of the frame), and a
//! grounded body walking into a slanted wall kept its velocity. The enemy fell
//! one tick ahead of Godot and turned one tick short.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;
use gdvariant::Variant;

/// One main-loop iteration, as the editor's Play and the Godot capture run it.
const FRAME: f64 = 1.0 / 60.0;

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer_3d")
        .canonicalize()
        .expect("games/platformer_3d fixture")
}

/// Boots `game.tscn` the way the runner does (same as
/// `platformer_3d_level_collision.rs`).
fn boot_game() -> MainLoop {
    gdobject::class_db::register_all_classes();
    let dir = game_dir();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let text = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    let settings = gdcore::project_settings::ProjectSettings::from_godot_format(&text).unwrap();
    gdscene::project_runtime::seed_script_settings_for_this_thread(&settings, &dir);
    let tscn = std::fs::read_to_string(dir.join("game.tscn")).unwrap();
    let packed = PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::begin_subscene_session();
    let id = gdscene::packed_scene::instance_packed_scene_detached(
        &mut tree,
        &packed,
        &gdscene::packed_scene::load_scene_from_res,
    )
    .unwrap();
    gdscene::scripting::attach_subtree_scripts_from_res(&mut tree, id);
    let node = tree.take_node(id).unwrap();
    tree.add_child(root, node).unwrap();
    gdscene::packed_scene::finish_attached_scene(&mut tree, id);
    let mut ml = MainLoop::new(tree);
    gdscene::project_runtime::apply_project_settings(&mut ml, &settings);
    ml.register_physics_bodies();
    ml
}

const PLAYER: &str = "/root/Game/Player";
const CAMERA: &str = "/root/Game/Player/Target/Camera3D";
const ENEMY: &str = "/root/Game/Enemies/Enemy1";
const ENEMY_SKELETON: &str = "/root/Game/Enemies/Enemy1/Enemy/Skeleton";
const ANIM_TREE: &str = "/root/Game/Player/AnimationTree";

/// Everything one frame of the scenario is checked against.
#[derive(Clone, Copy, Debug)]
struct Sample {
    player: Vector3,
    velocity: Vector3,
    camera: Vector3,
    enemy: Vector3,
    enemy_turn: f32,
    air_dir_blend: f64,
}

fn origin(ml: &MainLoop, path: &str) -> Vector3 {
    let id = ml.tree().get_node_by_path(path).unwrap_or_else(|| panic!("{path}"));
    gdscene::node3d::get_global_transform(ml.tree(), id).origin
}

fn prop(ml: &MainLoop, path: &str, key: &str) -> Variant {
    let id = ml.tree().get_node_by_path(path).unwrap_or_else(|| panic!("{path}"));
    ml.tree().get_node(id).unwrap().get_property(key)
}

fn sample(ml: &MainLoop) -> Sample {
    let velocity = match prop(ml, PLAYER, "velocity") {
        Variant::Vector3(v) => v,
        other => panic!("velocity is not a Vector3: {other:?}"),
    };
    let enemy_turn = match prop(ml, ENEMY_SKELETON, "rotation") {
        Variant::Vector3(v) => v.y,
        // An untouched skeleton has never been rotated.
        Variant::Nil => 0.0,
        other => panic!("Skeleton rotation is not a Vector3: {other:?}"),
    };
    let air_dir_blend = match prop(ml, ANIM_TREE, "parameters/air_dir/blend_amount") {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("air_dir blend is not a number: {other:?}"),
    };
    Sample {
        player: origin(ml, PLAYER),
        velocity,
        camera: origin(ml, CAMERA),
        // LOCAL `position`, as the Godot trace records it: `Enemies` is offset.
        enemy: {
            let id = ml.tree().get_node_by_path(ENEMY).unwrap();
            gdscene::node3d::get_position(ml.tree(), id)
        },
        enemy_turn,
        air_dir_blend,
    }
}

/// Runs the scenario and returns the sample after every frame (index = frame).
fn run_scenario(frames: usize) -> Vec<Sample> {
    let mut ml = boot_game();
    let mut out = vec![sample(&ml)];
    for f in 1..=frames {
        // Pressed before frame `start` steps, released after frame `end`.
        if f == 31 {
            ml.input_state_mut().action_press("move_forward", 1.0);
        }
        if f == 121 {
            ml.input_state_mut().action_release("move_forward");
            ml.input_state_mut().action_press("jump", 1.0);
        }
        if f == 139 {
            ml.input_state_mut().action_release("jump");
        }
        ml.step(FRAME);
        out.push(sample(&ml));
    }
    let errors = ml.tree_mut().drain_script_errors();
    assert!(errors.is_empty(), "script errors: {errors:?}");
    out
}

fn assert_near(actual: Vector3, godot: [f32; 3], tol: f32, what: &str) {
    let g = Vector3::new(godot[0], godot[1], godot[2]);
    let d = actual - g;
    assert!(
        d.x.abs() <= tol && d.y.abs() <= tol && d.z.abs() <= tol,
        "{what}: Patina {actual:?}, Godot {g:?} (tolerance {tol})"
    );
}

#[test]
fn player_camera_and_enemy_follow_godot_frame_by_frame() {
    let s = run_scenario(204);

    // -- frame 1: a body's first tick reads ZERO gravity (pat-d4ghb) ---------
    // Two ticks ran, but only the second fell: -22/120 = -0.183333.
    assert_near(s[1].velocity, [0.0, -0.183333, 0.0], 1e-4, "f1 player velocity");
    assert_near(s[1].player, [-9.49833, -3.843418, 3.9334], 1e-4, "f1 player");
    assert_near(s[1].camera, [-10.75505, 0.109022, 3.9334], 1e-4, "f1 camera");
    // The enemy's `_integrate_forces` ran twice (two 4/120 rad turns) but its
    // velocity moves it from the NEXT step, so it fell by one tick only.
    assert_near(s[1].enemy, [18.3062, 5.406742, 5.96938], 1e-4, "f1 enemy");
    assert!(
        (s[1].enemy_turn - 0.066667).abs() < 1e-4,
        "f1 enemy turn: Patina {}, Godot 0.066667",
        s[1].enemy_turn
    );

    // -- frame 8: landed exactly when Godot lands, not snapped early ---------
    assert_near(s[8].player, [-9.49833, -3.998966, 3.9334], 1e-3, "f8 player landed");
    assert_near(s[8].velocity, [0.0, 0.0, 0.0], 1e-4, "f8 player velocity");
    assert_near(s[8].camera, [-10.75505, -0.046525, 3.9334], 1e-3, "f8 camera");

    // -- frame 30: standing; the enemy's whole fall matches ------------------
    assert_near(s[30].enemy, [18.3062, 2.704103, 5.96938], 1e-4, "f30 enemy falling");

    // -- walking forward ------------------------------------------------------
    assert_near(s[60].player, [-7.754435, -3.999599, 3.9334], 1e-3, "f60 player");
    assert_near(s[60].velocity, [6.066667, 0.0, 0.0], 1e-4, "f60 velocity");
    // ⚠️ 2.5e-3: the camera integrates the player's resting height, which Jolt
    // leaves ~6e-4 lower than Patina while walking (see the report).
    assert_near(s[60].camera, [-10.667976, -0.107758, 3.9334], 2.5e-3, "f60 camera");
    assert_near(s[95].player, [-4.215555, -3.999599, 3.933393], 1e-3, "f95 player");
    assert_near(s[95].camera, [-7.630732, -1.281284, 3.9334], 2.5e-3, "f95 camera");

    // -- over the ledge: the vertical-only floor snap keeps it on the floor
    //    as long as Godot does, with no sideways shove --------------------------
    assert_near(s[99].player, [-3.810835, -4.108875, 3.9334], 2e-3, "f99 player");
    assert_near(s[99].velocity, [6.066667, 0.0, 0.0], 1e-4, "f99 velocity (still grounded)");
    assert_near(s[120].player, [-1.709782, -5.488476, 3.933331], 2e-3, "f120 player");

    // -- the jump release cuts the jump ONCE ----------------------------------
    assert_near(s[139].velocity, [5.883332, 7.668333, -0.004018], 1e-3, "f139 velocity");
    assert_near(s[160].player, [2.240675, -5.868292, 3.93064], 2e-3, "f160 player");

    // -- a grounded body walking into a slanted wall slides its velocity -----
    assert_near(s[198].velocity, [0.610251, 0.0, 1.220503], 1e-3, "f198 velocity");
    assert_near(s[204].velocity, [0.0, 0.0, 0.0], 1e-4, "f204 velocity");

    // -- the enemy walks Godot's path ----------------------------------------
    // x/z within 1e-3. y is 0.0103 higher: Jolt rests a body inside its
    // `penetration_slop` (0.02); with the project's slop set to 0 Godot rests
    // it at 1.9381, exactly where Patina does.
    let e = s[60].enemy;
    assert!(
        (e.x - 18.750597).abs() < 1e-3 && (e.z - 5.689173).abs() < 1e-3,
        "f60 enemy x/z: Patina {e:?}, Godot (18.750597, _, 5.689173)"
    );
    assert!((e.y - 1.9381).abs() < 1e-3, "f60 enemy rest height {}", e.y);
}

/// pat-hwhbj: the player script's line right after `move_and_slide()` —
/// `air_dir/blend_amount = clampf(-velocity.y / 4 + 0.5, 0, 1)` — sees the
/// POST-slide velocity. Standing on the floor Godot's is exactly 0.5
/// (velocity.y == 0). With the slide deferred to the next tick the script read
/// the pre-slide -0.1833 and wrote 0.5458.
#[test]
fn velocity_read_right_after_move_and_slide_is_the_slid_one() {
    let s = run_scenario(30);
    for f in [10, 20, 30] {
        assert!(
            (s[f].air_dir_blend - 0.5).abs() < 1e-6,
            "f{f} air_dir blend: Patina {}, Godot 0.5",
            s[f].air_dir_blend
        );
    }
}
