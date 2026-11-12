//! pat-dt40j — the official 2D Platformer (and Dodge the Creeps) against REAL
//! Godot 4.6.1, frame by frame.
//!
//! The numbers below were captured from Godot.app with
//! `node scripts/godot-reference.mjs games/<g> --watch ...` on the games'
//! unmodified `games/*` copies, under each game's `playtest.json` timeline
//! (60 fps, inputs pressed before frame `start` and released after frame
//! `end`, exactly as the editor's MCP `play` drives Patina). Frame f is the
//! state after f main-loop iterations; the platformer runs TWO physics ticks
//! per frame (`physics_ticks_per_second = 120`).
//!
//! What each checkpoint pins (each was red before its fix):
//! * f1  y = 636.6458 — `get_gravity()` is ZERO until a body's first physics
//!   step (Godot computes gravity inside the step), so only ONE of the frame's
//!   two ticks falls. Patina fell both: 636.75.
//! * f2, f70 — the resting / walking height (636.7495 / 636.7479) comes out of
//!   Godot's `test_body_motion` (recovery, margin cast, rest contact) and
//!   Godot's MERGED tile rows; the old slide rested at 636.75 and walked at
//!   600 px/s (its `floor_constant_speed` stretched the gravity motion into the
//!   floor into extra horizontal travel).
//! * f147..f210 — the player pushes against Enemy3 (a kinematic capsule rotated
//!   90°) and stops at x = 500.688; the enemy walks at 22 px/s. Patina used to
//!   run past it at x = 552.8, the enemy at twice its speed.
//! * f151 — the jump: `is_action_just_pressed` holds for ONE physics tick, so
//!   only the first tick sets `velocity.y = -725`. Patina re-jumped in the
//!   frame's second tick and rose 0.146 px higher.
//! * f300 — x = 74.438 after 90 frames of ArrowLeft: no early stop at a left
//!   boundary (the old -106.75 was the 2x speed reaching the level's wall).

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, Key};
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;
use gdvariant::Variant;

/// `set_path_roots` is process-global; tests in this binary take turns.
static ROOTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn game_dir(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games")
        .join(name)
        .canonicalize()
        .expect("games fixture")
}

/// Boots `scene` the way editor Play does: project settings applied, the
/// project's `[input]` map installed, physics bodies registered.
fn boot_game(name: &str, scene: &str) -> MainLoop {
    gdobject::class_db::register_all_classes();
    let dir = game_dir(name);
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let text = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    let settings = gdcore::project_settings::ProjectSettings::from_godot_format(&text).unwrap();
    gdscene::project_runtime::seed_script_settings_for_this_thread(&settings, &dir);
    let tscn = std::fs::read_to_string(dir.join(scene)).unwrap();
    let packed = PackedScene::from_tscn(&tscn).unwrap();
    // The window Godot opens (`window/size/viewport_*`) exists before any
    // `_ready` — Dodge the Creeps reads `get_viewport_rect().size` there.
    let dim = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(k))
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap()
    };
    let (vw, vh) = (
        dim("window/size/viewport_width="),
        dim("window/size/viewport_height="),
    );
    let mut tree = SceneTree::new();
    tree.resize_root_window(Vector2::new(vw as f32, vh as f32));
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
    ml.set_input_map(gdplatform::InputMap::load_from_project_godot(&text));
    // ...and the editor publishes its game view every frame.
    ml.set_viewport_size(vw, vh);
    ml.register_physics_bodies();
    ml
}

fn pos(ml: &MainLoop, path: &str) -> Vector2 {
    let id = ml
        .tree()
        .get_node_by_path(path)
        .unwrap_or_else(|| panic!("{path}"));
    match ml.tree().get_node(id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        o => panic!("{path}:position is {o:?}"),
    }
}

fn key(ml: &mut MainLoop, key: Key, pressed: bool) {
    ml.push_event(InputEvent::Key {
        key,
        pressed,
        shift: false,
        ctrl: false,
        alt: false,
    });
}

/// A key held from frame `start` through frame `end` (inclusive).
struct Hold {
    key: Key,
    start: u32,
    end: u32,
}

/// Steps `frames` frames at 60 fps with `holds`, calling `probe` after each.
fn run(ml: &mut MainLoop, frames: u32, holds: &[Hold], mut probe: impl FnMut(u32, &MainLoop)) {
    for f in 1..=frames {
        for h in holds.iter().filter(|h| h.start == f) {
            key(ml, h.key, true);
        }
        ml.step(1.0 / 60.0);
        probe(f, ml);
        for h in holds.iter().filter(|h| h.end == f) {
            key(ml, h.key, false);
        }
    }
}

fn assert_near(what: &str, frame: u32, patina: Vector2, godot: (f32, f32)) {
    let d = (patina.x - godot.0).abs().max((patina.y - godot.1).abs());
    assert!(
        d <= 0.01,
        "{what} at f{frame}: Patina {patina:?}, Godot ({}, {}) — off by {d}",
        godot.0,
        godot.1
    );
}

/// `games/platformer/playtest.json`: wait:1000, ArrowRight:1500, ArrowUp:300,
/// wait:700, ArrowLeft:1500.
#[test]
fn platformer_player_and_enemy_follow_godot_frame_by_frame() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut ml = boot_game("platformer", "game_singleplayer.tscn");
    assert_eq!(ml.physics_ticks_per_second(), 120);
    // (frame, player, enemy3) — Godot 4.6.1.
    let godot: &[(u32, (f32, f32), (f32, f32))] = &[
        (1, (90.0, 636.645_8), (492.366_64, 610.145_8)),
        (2, (90.0, 636.749_5), (492.733_28, 610.875)),
        (61, (90.375, 636.745_6), (514.368_1, 631.925_2)),
        (70, (116.25, 636.747_9), (517.668_4, 631.925_2)),
        (147, (499.458_68, 636.789_25), (545.904_24, 631.925_2)),
        (150, (500.549_38, 636.691_77), (547.004_33, 631.925_2)),
        (151, (500.688_1, 625.045_96), (547.371_03, 631.925_2)),
        (152, (500.688_1, 613.983_46), (547.737_73, 631.925_2)),
        (168, (500.688_1, 578.262_27), (553.604_9, 631.925_2)),
        (210, (500.688_1, 636.744_87), (569.006_3, 631.925_2)),
        (211, (500.313_1, 636.745_6), (569.373, 631.925_2)),
        (300, (74.438_11, 636.747_9), (602.009_2, 631.925_2)),
    ];
    let holds = [
        Hold {
            key: Key::Right,
            start: 61,
            end: 150,
        },
        Hold {
            key: Key::Up,
            start: 151,
            end: 168,
        },
        Hold {
            key: Key::Left,
            start: 211,
            end: 300,
        },
    ];
    run(&mut ml, 300, &holds, |f, ml| {
        if let Some((_, player, enemy)) = godot.iter().find(|g| g.0 == f) {
            assert_near("Player", f, pos(ml, "/root/Game/Level/Player"), *player);
            assert_near(
                "Enemy3",
                f,
                pos(ml, "/root/Game/Level/Enemies/Enemy3"),
                *enemy,
            );
        }
    });
    let errors = ml.tree_mut().drain_script_errors();
    assert!(errors.is_empty(), "script errors: {errors:?}");
}

/// The platformer's Camera2D (`player.tscn`: position (0, -28), offset
/// (0, 50); `level.gd` sets limit_bottom 690) frames the view like Godot.
///
/// Godot clamps the view to the limits and THEN adds `offset`
/// (`camera_2d.cpp:253`), so the bottom limit at 690 leaves the screen centre
/// at y = 690 - 240 + 50 = 500 for a player resting at 636.75. Patina added
/// the offset first and clamped it away: centre 450, the whole level drawn
/// 50 px low (58% of pixels differed from Godot's frame).
#[test]
fn platformer_camera_adds_its_offset_after_the_limits() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut ml = boot_game("platformer", "game_singleplayer.tscn");
    run(&mut ml, 60, &[], |_, _| {});
    let center = gdscene::canvas_render::camera2d_center(
        ml.tree(),
        Vector2::new(800.0, 480.0),
        None,
        1.0 / 60.0,
    )
    .expect("the player's camera is current");
    assert!(
        (center.x - 90.0).abs() < 1e-3 && (center.y - 500.0).abs() < 1e-3,
        "screen centre {center:?}, Godot (90, 500)"
    );
}

/// `games/dodge_the_creeps/playtest.json`: Enter:300, wait:2500,
/// ArrowRight:1500, ArrowUp:1000.
///
/// The Start button's shortcut is an `InputEventAction` (`start_game`, bound
/// to Enter). On frame 1 the shortcut stage ran before the engine had
/// published the input-map mirror it matches against, so the Enter was
/// ignored and the player stayed hidden at (0, 0); Godot starts it at the
/// StartPosition (240, 450) on frame 1.
#[test]
fn dodge_the_creeps_starts_on_a_frame_one_enter_like_godot() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut ml = boot_game("dodge_the_creeps", "main.tscn");
    let godot: &[(u32, (f32, f32))] = &[
        (1, (240.0, 450.0)),
        (18, (240.0, 450.0)),
        (170, (253.333_34, 450.0)),
        (258, (480.0, 450.0)),
        (260, (480.0, 436.666_7)),
        (318, (480.0, 50.000_2)),
    ];
    let holds = [
        Hold {
            key: Key::Enter,
            start: 1,
            end: 18,
        },
        Hold {
            key: Key::Right,
            start: 169,
            end: 258,
        },
        Hold {
            key: Key::Up,
            start: 259,
            end: 318,
        },
    ];
    run(&mut ml, 318, &holds, |f, ml| {
        if let Some((_, p)) = godot.iter().find(|g| g.0 == f) {
            assert_near("Player", f, pos(ml, "/root/Main/Player"), *p);
        }
    });
}

/// `games/pong/playtest.json`: wait:800, KeyW:1000, KeyS:1000, ArrowUp:1000.
///
/// The ball (an Area2D moved in `_process`) enters LeftWall during frame 185.
/// Godot emits a step's `area_entered` at the START of the next tick
/// (`flush_queries` before `physics_process`), so the wall resets the ball in
/// frame 187: f186 is still x = 0.838. Patina emitted the signal right after
/// the step and reset it a frame early (f186 x = 318.8).
#[test]
fn pong_wall_resets_the_ball_on_godots_frame() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut ml = boot_game("pong", "pong.tscn");
    let godot: &[(u32, (f32, f32), (f32, f32))] = &[
        (108, (137.23, 191.124), (67.628_5, 92.593_86)),
        (185, (2.608_334_5, 191.124), (67.628_5, 192.594)),
        (186, (0.838_334_6, 191.124), (67.628_5, 192.594)),
        (187, (318.832_76, 191.124), (67.628_5, 192.594)),
        (228, (249.998_34, 191.124), (67.628_5, 192.594)),
    ];
    let holds = [
        Hold {
            key: Key::W,
            start: 49,
            end: 108,
        },
        Hold {
            key: Key::S,
            start: 109,
            end: 168,
        },
        Hold {
            key: Key::Up,
            start: 169,
            end: 228,
        },
    ];
    run(&mut ml, 228, &holds, |f, ml| {
        if let Some((_, ball, left)) = godot.iter().find(|g| g.0 == f) {
            assert_near("Ball", f, pos(ml, "/root/Pong/Ball"), *ball);
            assert_near("Left", f, pos(ml, "/root/Pong/Left"), *left);
        }
    });
}
