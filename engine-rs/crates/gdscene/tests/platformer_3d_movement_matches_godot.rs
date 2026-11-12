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
    enemy_velocity: Vector3,
    enemy_turn: f32,
    air_dir_blend: f64,
}

fn origin(ml: &MainLoop, path: &str) -> Vector3 {
    let id = ml
        .tree()
        .get_node_by_path(path)
        .unwrap_or_else(|| panic!("{path}"));
    gdscene::node3d::get_global_transform(ml.tree(), id).origin
}

fn prop(ml: &MainLoop, path: &str, key: &str) -> Variant {
    let id = ml
        .tree()
        .get_node_by_path(path)
        .unwrap_or_else(|| panic!("{path}"));
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
        enemy_velocity: match prop(ml, ENEMY, "linear_velocity") {
            Variant::Vector3(v) => v,
            _ => Vector3::ZERO,
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
    assert_near(
        s[1].velocity,
        [0.0, -0.183333, 0.0],
        1e-4,
        "f1 player velocity",
    );
    assert_near(
        s[1].player,
        [-9.49833, -3.843418, 3.9334],
        1e-4,
        "f1 player",
    );
    assert_near(
        s[1].camera,
        [-10.75505, 0.109022, 3.9334],
        1e-4,
        "f1 camera",
    );
    // pat-29zst: Godot copies a rigid body's solved state onto the node at the
    // NEXT tick's flush, and has no step before the first tick. So after two
    // ticks the enemy has had ONE `_integrate_forces` (one 4/120 rad turn,
    // velocity -0.1833) and still reads its authored height; after four it has
    // three callbacks and has moved by two steps. (The d4ghb numbers here,
    // 5.406742 and two turns, were one tick ahead of Godot.)
    assert_near(
        s[1].enemy,
        [18.3062, 5.40827, 5.96938],
        1e-5,
        "f1 enemy (not yet moved)",
    );
    assert!(
        (s[1].enemy_turn - 0.033333).abs() < 1e-4,
        "f1 enemy turn: Patina {}, Godot 0.033333",
        s[1].enemy_turn
    );
    assert_near(s[2].enemy, [18.3062, 5.403687, 5.96938], 1e-5, "f2 enemy");
    assert!(
        (s[2].enemy_turn - 0.1).abs() < 1e-4,
        "f2 enemy turn: Patina {}, Godot 0.1",
        s[2].enemy_turn
    );

    // -- frame 8: landed exactly when Godot lands, not snapped early ---------
    assert_near(
        s[8].player,
        [-9.49833, -3.998966, 3.9334],
        1e-5,
        "f8 player landed",
    );
    assert_near(s[8].velocity, [0.0, 0.0, 0.0], 1e-4, "f8 player velocity");
    assert_near(
        s[8].camera,
        [-10.75505, -0.046525, 3.9334],
        1e-5,
        "f8 camera",
    );

    // -- frame 30: standing; the enemy's whole fall matches ------------------
    assert_near(
        s[30].enemy,
        [18.3062, 2.794241, 5.96938],
        1e-4,
        "f30 enemy falling",
    );

    // -- walking forward ------------------------------------------------------
    // pat-kiuul: 1e-5, from 1e-3 / 2.5e-3. The walking body rests where Jolt's
    // motion query rests it (-3.999599 over this tile, 4e-4 above its top;
    // -3.999897 over a plain box), not at floor + safe_margin as the old
    // push-out solver did, and the camera that integrates it follows.
    assert_near(
        s[60].player,
        [-7.754435, -3.999599, 3.9334],
        1e-5,
        "f60 player",
    );
    assert_near(s[60].velocity, [6.066667, 0.0, 0.0], 1e-5, "f60 velocity");
    assert_near(
        s[60].camera,
        [-10.667976, -0.107758, 3.9334],
        1e-5,
        "f60 camera",
    );
    assert_near(
        s[95].player,
        [-4.215555, -3.999599, 3.933393],
        1e-5,
        "f95 player",
    );
    assert_near(
        s[95].camera,
        [-7.630732, -1.281284, 3.9334],
        1e-5,
        "f95 camera",
    );

    // -- over the ledge: the vertical-only floor snap keeps it on the floor
    //    as long as Godot does, with no sideways shove --------------------------
    assert_near(
        s[99].player,
        [-3.810835, -4.108875, 3.9334],
        1e-5,
        "f99 player",
    );
    assert_near(
        s[99].velocity,
        [6.066667, 0.0, 0.0],
        1e-5,
        "f99 velocity (still grounded)",
    );
    assert_near(
        s[120].player,
        [-1.709782, -5.488476, 3.933331],
        1e-5,
        "f120 player",
    );

    // -- the jump release cuts the jump ONCE ----------------------------------
    assert_near(
        s[139].velocity,
        [5.883332, 7.668333, -0.004018],
        1e-5,
        "f139 velocity",
    );
    assert_near(
        s[160].player,
        [2.240675, -5.868292, 3.93064],
        1e-5,
        "f160 player",
    );

    // -- a grounded body walking into a slanted wall slides its velocity -----
    assert_near(
        s[198].velocity,
        [0.610251, 0.0, 1.220503],
        1e-5,
        "f198 velocity",
    );
    assert_near(s[204].velocity, [0.0, 0.0, 0.0], 1e-5, "f204 velocity");

    // -- the enemy lands and rests where Jolt rests it (pat-29zst) ----------
    // Jolt corrects only the penetration beyond its `penetration_slop` (0.02),
    // so the enemy, which falls 0.0101 m into the floor on landing, stays that
    // deep: Godot y = 1.92799 from f35. A full push-out rested it on the
    // surface at 1.9381.
    assert_near(
        s[34].enemy,
        [18.30708, 2.03035, 5.96882],
        1e-3,
        "f34 enemy, last frame airborne",
    );
    for f in [35, 40, 50] {
        let y = s[f].enemy.y;
        assert!(
            (y - 1.92799).abs() < 2e-4,
            "f{f} enemy rest height: Patina {y}, Godot 1.92799"
        );
    }

    // -- the enemy walks Godot's path ----------------------------------------
    // Frame-exact (1e-4) through f56, including over the seam between two
    // GridMap cells at f51, which a slop-deep body used to climb (see
    // `internal_edge_ghosts` in gdphysics3d). From f57 Jolt reports one more
    // contact event on the next cell (velocity 1.727 -> 1.694, y dips 0.0009)
    // that is not modelled, so later frames are only within 2e-3.
    assert_near(
        s[45].enemy,
        [18.394316, 1.927991, 5.913824],
        1e-4,
        "f45 enemy walking",
    );
    assert_near(
        s[45].enemy_velocity,
        [0.881151, -0.183333, -0.555556],
        1e-4,
        "f45 enemy velocity",
    );
    assert_near(
        s[56].enemy,
        [18.623709, 1.927991, 5.769195],
        1e-4,
        "f56 enemy over the seam",
    );
    assert_near(
        s[56].enemy_velocity,
        [1.656563, -0.183333, -1.044446],
        1e-4,
        "f56 enemy velocity",
    );
    assert_near(
        s[60].enemy,
        [18.736485, 1.927502, 5.69806],
        2e-3,
        "f60 enemy",
    );
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

/// pat-kiuul: the parity gate's own scenario (`games/platformer_3d/
/// playtest.json`, keys `wait:1500,KeyW:1500,KeyA:1000,KeyD:1000,Space:300`:
/// W for frames 91-180, A for 181-240) against the frame-exact Godot 4.6.1
/// capture of `scripts/compare-to-godot.mjs`.
///
/// Before the Jolt `body_test_motion` port the walking player rested at
/// floor + safe_margin (-3.9990 where Godot reads -3.999901), missed the
/// 1.4e-3 bump at the x = -8 GridMap seam (f118), and lost horizontal speed
/// differently on the f195 landing — the gate's first divergence at f237.
#[test]
fn gate_scenario_matches_godot_through_the_seam_and_the_landing() {
    let mut ml = boot_game();
    let mut s = vec![sample(&ml)];
    for f in 1..=196 {
        let input = ml.input_state_mut();
        if f == 91 {
            input.action_press("move_forward", 1.0);
        }
        if f == 181 {
            input.action_release("move_forward");
            input.action_press("move_left", 1.0);
        }
        ml.step(FRAME);
        s.push(sample(&ml));
    }
    // Walking off from rest: Jolt's 5-step cast leaves the capsule 1e-4 above
    // the floor box, not at the 1e-3 safe margin.
    assert_near(
        s[92].player,
        [-9.48861, -3.999949, 3.9334],
        1e-5,
        "f92 player",
    );
    assert_near(
        s[100].player,
        [-9.294164, -3.999901, 3.9334],
        1e-5,
        "f100 player",
    );
    // The seam onto the next tile at x = -8: up 1.4e-3, then 4e-4 above it.
    assert_near(
        s[118].player,
        [-7.956657, -3.998502, 3.9334],
        1e-5,
        "f118 player over the seam",
    );
    assert_near(
        s[119].player,
        [-7.855546, -3.99959, 3.9334],
        1e-5,
        "f119 player",
    );
    assert_near(
        s[160].player,
        [-3.70978, -4.113474, 3.9334],
        1e-5,
        "f160 player off the ledge",
    );
    // The landing: position and the velocity it keeps.
    assert_near(
        s[195].player,
        [-0.234852, -7.999715, 3.704301],
        1e-5,
        "f195 player landed",
    );
    assert_near(
        s[195].velocity,
        [5.597527, 0.0, -2.160486],
        1e-5,
        "f195 velocity",
    );
    // ⚠️ The landing height settles 7e-6 off Godot at f196: Jolt's cast
    // brackets a touch within 2e-7 of its f32 rounding there, and which side
    // it falls is float noise. From f197 the player script's
    // `horizontal_speed < MAX_SPEED` meets a speed equal to 6 within an ulp,
    // so the run is chaotic past this frame.
    assert_near(
        s[196].player,
        [-0.146606, -7.999895, 3.657423],
        1e-5,
        "f196 player",
    );
    assert_near(
        s[196].velocity,
        [5.185974, 0.0, -3.017562],
        1e-5,
        "f196 velocity",
    );
}
