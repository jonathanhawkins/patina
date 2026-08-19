//! pat-s3rrv.46 acceptance: `Tween` pause/process modes are applied by the main
//! loop, and the `Tween`/`PropertyTweener` script surface is complete.
//!
//! Before this bead:
//!
//! * `Tween::can_process` was a faithful port of Godot's
//!   `scene/animation/tween.cpp` with **zero production callers** —
//!   `SceneTree::process_tweens` advanced every tween unconditionally, and
//!   `MainLoop::step` skipped the whole animation/tween phase under pause. A
//!   `TWEEN_PAUSE_PROCESS` pause-menu tween therefore froze with the game, and
//!   a `TWEEN_PROCESS_PHYSICS` tween ran at render rate.
//! * `from`, `from_current`, `as_relative`, `tween_subtween`, `bind_node`,
//!   `custom_step`, `set_process_mode`, `set_pause_mode`,
//!   `set_ignore_time_scale`, `get_total_elapsed_time`, `interpolate_value` and
//!   `get_loops_left` existed **only** as ClassDB `MethodInfo` entries, so
//!   `ClassDB.class_has_method("Tween", "tween_subtween")` answered true while
//!   calling it raised `UndefinedFunction`.
//! * `step_finished` / `loop_finished` were declared and never emitted.
//!
//! Every tween here is built by the scene's own GDScript and driven by real
//! `MainLoop::step` frames through `EngineBootstrap` — no `SceneTree::add_tween`
//! and no hand-built `Tween` anywhere in this file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `boot_project` mutates the process-global `res://` path roots, so serialize
/// these tests to stay correct under `cargo test` (one process, many threads).
/// `cargo nextest` isolates each test in its own process and doesn't need this.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_s3rrv46_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `A` starts at x=25 so `as_relative` has a start value that is not zero — the
/// whole point of the verb is that it ends at start+delta, and a start of 0
/// would make the relative and absolute answers identical.
const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="A" type="Sprite2D" parent="."]
position = Vector2(25, 0)

[node name="B" type="Sprite2D" parent="."]
position = Vector2(0, 0)
"#;

fn write_project(dir: &Path, tscn: &str, gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), tscn).unwrap();
    std::fs::write(dir.join("main.gd"), gd).unwrap();
}

fn boot(dir: &Path) -> EngineBootstrap {
    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");
    boot
}

/// Reads a script variable's current value from a node.
fn script_var(tree: &gdscene::SceneTree, node_id: gdscene::node::NodeId, var: &str) -> Variant {
    tree.get_script(node_id)
        .expect("node has a script")
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
        .unwrap_or(Variant::Nil)
}

fn as_int(v: Variant) -> i64 {
    match v {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        other => panic!("expected a number, got {other:?}"),
    }
}

fn node_x(tree: &gdscene::SceneTree, path: &str) -> f32 {
    let id = tree
        .get_node_by_path(path)
        .unwrap_or_else(|| panic!("{path} instanced"));
    match tree.get_node(id).unwrap().get_property("position") {
        Variant::Vector2(v) => v.x,
        other => panic!("{path}.position should be a Vector2, got {other:?}"),
    }
}

fn main_id(boot: &EngineBootstrap) -> gdscene::node::NodeId {
    boot.main_loop()
        .expect("main loop built")
        .tree()
        .get_node_by_path("/root/Main")
        .expect("main scene instanced")
}

// ---------------------------------------------------------------------------
// 1. Pause modes
// ---------------------------------------------------------------------------

/// Two tweens, identical except for their pause mode, under a paused tree.
const PAUSE_GD: &str = "\
extends Node2D

func _ready():
    var menu = create_tween()
    menu.set_pause_mode(Tween.TWEEN_PAUSE_PROCESS)
    menu.tween_property($A, \"position:x\", 125.0, 1.0)
    var game = create_tween()
    game.tween_property($B, \"position:x\", 100.0, 1.0)
";

/// **The pause acceptance test.** With `get_tree().paused = true`, a
/// `TWEEN_PAUSE_PROCESS` tween keeps advancing its target property across
/// `MainLoop::step` frames while a default (`TWEEN_PAUSE_BOUND`, owner not
/// `PROCESS_MODE_ALWAYS`) tween does not.
///
/// Both halves matter: before this bead the whole phase sat inside
/// `MainLoop`'s `if !paused` guard, so BOTH tweens froze — asserting only that
/// the bound one holds still would have passed against the bug.
#[test]
fn pause_mode_process_advances_under_pause_and_bound_does_not() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("pause");
    write_project(&dir, MAIN_TSCN, PAUSE_GD);

    let mut boot = boot(&dir);
    assert_eq!(
        boot.main_loop().unwrap().tree().tween_count(),
        2,
        "the script built both tweens through the bridge"
    );

    // Pause BEFORE any frame runs, so neither tween has advanced yet.
    boot.main_loop_mut().unwrap().tree_mut().set_paused(true);
    assert_eq!(
        node_x(boot.main_loop().unwrap().tree(), "/root/Main/A"),
        25.0
    );
    assert_eq!(
        node_x(boot.main_loop().unwrap().tree(), "/root/Main/B"),
        0.0
    );

    for _ in 0..10 {
        boot.main_loop_mut().unwrap().step(1.0 / 60.0);
    }

    let tree = boot.main_loop().unwrap().tree();
    let menu_x = node_x(tree, "/root/Main/A");
    assert!(
        menu_x > 25.0,
        "TWEEN_PAUSE_PROCESS must keep advancing while the tree is paused, \
         but A sat at its start value {menu_x}"
    );
    assert_eq!(
        node_x(tree, "/root/Main/B"),
        0.0,
        "a default TWEEN_PAUSE_BOUND tween on a Pausable owner must freeze under pause"
    );

    // Unpausing resumes the bound tween — the freeze above is pause, not a dead
    // tween.
    boot.main_loop_mut().unwrap().tree_mut().set_paused(false);
    for _ in 0..10 {
        boot.main_loop_mut().unwrap().step(1.0 / 60.0);
    }
    assert!(
        node_x(boot.main_loop().unwrap().tree(), "/root/Main/B") > 0.0,
        "unpausing must resume the bound tween"
    );
}

// ---------------------------------------------------------------------------
// 2. Process mode (idle vs physics phase)
// ---------------------------------------------------------------------------

/// `A` advances on the physics tick, `B` on the idle frame. Same duration, same
/// distance — the only difference is the phase.
const PROCESS_MODE_GD: &str = "\
extends Node2D

func _ready():
    var phys = create_tween()
    phys.set_process_mode(Tween.TWEEN_PROCESS_PHYSICS)
    phys.tween_property($A, \"position:x\", 85.0, 1.0)
    var idle = create_tween()
    idle.set_process_mode(Tween.TWEEN_PROCESS_IDLE)
    idle.tween_property($B, \"position:x\", 60.0, 1.0)
";

/// **The process-mode acceptance test.** A `TWEEN_PROCESS_PHYSICS` tween
/// advances exactly once per physics tick.
///
/// The discriminator is a frame SHORTER than one physics tick: at 60 tps a
/// `1/120` step accumulates half a tick and runs the physics loop zero times, so
/// a physics-mode tween must not have moved at all while the idle-mode control
/// has. A second such frame completes the tick and the physics tween jumps by
/// exactly one tick's worth (`1/60` of a 60px-per-second linear ramp = 1.0px),
/// not by the two render frames' worth the idle tween covered.
#[test]
fn process_mode_physics_advances_once_per_physics_tick() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("process_mode");
    write_project(&dir, MAIN_TSCN, PROCESS_MODE_GD);

    let mut boot = boot(&dir);
    // Half a physics tick: the idle phase runs, the physics loop does not.
    boot.main_loop_mut().unwrap().step(1.0 / 120.0);

    let tree = boot.main_loop().unwrap().tree();
    assert_eq!(
        node_x(tree, "/root/Main/A"),
        25.0,
        "a TWEEN_PROCESS_PHYSICS tween must not advance on a frame that runs no \
         physics tick — it ran at render rate before this bead"
    );
    let idle_after_half = node_x(tree, "/root/Main/B");
    assert!(
        idle_after_half > 0.0,
        "the TWEEN_PROCESS_IDLE control must advance on that same frame \
         (got {idle_after_half}) — otherwise the assertion above is vacuous"
    );

    // Second half: the accumulator reaches one tick, so the physics tween moves
    // by exactly one tick of a 60px/s ramp.
    boot.main_loop_mut().unwrap().step(1.0 / 120.0);
    let moved = node_x(boot.main_loop().unwrap().tree(), "/root/Main/A") - 25.0;
    assert!(
        (moved - 1.0).abs() < 1e-3,
        "one physics tick of a 60px-per-second tween is 1.0px, got {moved}"
    );
}

// ---------------------------------------------------------------------------
// 3. PropertyTweener.as_relative
// ---------------------------------------------------------------------------

const RELATIVE_GD: &str = "\
extends Node2D

func _ready():
    var t = create_tween()
    t.tween_property($A, \"position\", Vector2(10, 0), 1.0).as_relative()
";

/// **The `as_relative` acceptance test.** `tween_property(node, \"position\",
/// Vector2(10, 0), 1.0).as_relative()` ends at start+10, not at 10.
///
/// `A` starts at x=25, so the two answers are 35 and 10 — a start of 0 would
/// make this test pass either way. `as_relative` raised `UndefinedFunction`
/// before this bead, so `_ready` itself used to fail here.
#[test]
fn as_relative_ends_at_start_plus_offset() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("relative");
    write_project(&dir, MAIN_TSCN, RELATIVE_GD);

    let mut boot = boot(&dir);
    assert_eq!(
        boot.main_loop().unwrap().tree().tween_count(),
        1,
        "as_relative() must not break the fluent chain — the tween exists"
    );

    for _ in 0..15 {
        boot.main_loop_mut().unwrap().step(0.1);
    }
    assert_eq!(
        node_x(boot.main_loop().unwrap().tree(), "/root/Main/A"),
        35.0,
        "as_relative() makes the final value an OFFSET from the start (25 + 10)"
    );
}

// ---------------------------------------------------------------------------
// 4. tween_subtween
// ---------------------------------------------------------------------------

const SUBTWEEN_GD: &str = "\
extends Node2D

func _ready():
    var sub = create_tween()
    sub.tween_property($B, \"position:x\", 50.0, 0.5)
    var t = create_tween()
    t.tween_property($A, \"position:x\", 45.0, 0.5)
    t.tween_subtween(sub)
";

/// **The `tween_subtween` acceptance test.** A nested tween runs to completion
/// as one step of its parent.
///
/// `TweenStep::subtween` existed and was unreachable: `tween_subtween` was a
/// ClassDB entry only. The parent takes ownership, so the tree holds one tween
/// rather than two once the chain is built.
#[test]
fn tween_subtween_runs_the_nested_tween_as_one_step() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("subtween");
    write_project(&dir, MAIN_TSCN, SUBTWEEN_GD);

    let mut boot = boot(&dir);
    assert_eq!(
        boot.main_loop().unwrap().tree().tween_count(),
        1,
        "the parent takes ownership of the subtween, so only it is left in the tree"
    );

    // Through the parent's own step only: the nested one must still be waiting.
    for _ in 0..5 {
        boot.main_loop_mut().unwrap().step(0.1);
    }
    let tree = boot.main_loop().unwrap().tree();
    assert!(
        node_x(tree, "/root/Main/A") > 25.0,
        "the parent's first step drives A"
    );
    assert!(
        node_x(tree, "/root/Main/B") < 50.0,
        "the nested tween is a SEQUENTIAL step — it must not be finished yet"
    );

    // Past the nested step's end.
    for _ in 0..10 {
        boot.main_loop_mut().unwrap().step(0.1);
    }
    let tree = boot.main_loop().unwrap().tree();
    assert_eq!(
        node_x(tree, "/root/Main/A"),
        45.0,
        "the parent's own step settles on its final value"
    );
    assert_eq!(
        node_x(tree, "/root/Main/B"),
        50.0,
        "the nested tween ran to completion as one step of the parent"
    );
}

// ---------------------------------------------------------------------------
// 5. loop_finished / step_finished
// ---------------------------------------------------------------------------

const LOOPS_GD: &str = "\
extends Node2D
var loops_seen = 0
var last_loop = 0
var steps_seen = 0

func _ready():
    var t = create_tween()
    t.set_loops(3)
    t.tween_property($A, \"position:x\", 35.0, 0.1)
    t.connect(\"loop_finished\", self, \"_on_loop\")
    t.connect(\"step_finished\", self, \"_on_step\")

func _on_loop(count):
    loops_seen += 1
    last_loop = count

func _on_step(idx):
    steps_seen += 1
";

/// **The signal acceptance test.** `loop_finished` fires once per loop.
///
/// Both signals were declared in ClassDB and emitted by nothing at all.
#[test]
fn loop_finished_and_step_finished_reach_the_script() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("loops");
    write_project(&dir, MAIN_TSCN, LOOPS_GD);

    let mut boot = boot(&dir);
    let main = main_id(&boot);

    // A 0.1s single-step tween looping 3 times: 0.5s covers all three loops.
    for _ in 0..10 {
        boot.main_loop_mut().unwrap().step(0.05);
    }

    let tree = boot.main_loop().unwrap().tree();
    assert_eq!(
        as_int(script_var(tree, main, "loops_seen")),
        3,
        "loop_finished fires once per loop of a set_loops(3) tween"
    );
    assert_eq!(
        as_int(script_var(tree, main, "last_loop")),
        3,
        "loop_finished carries the running loop count, like Godot"
    );
    assert_eq!(
        as_int(script_var(tree, main, "steps_seen")),
        3,
        "step_finished fires as the single tweener completes on each loop"
    );
}

// ---------------------------------------------------------------------------
// 6. The rest of the surface that was ClassDB-only
// ---------------------------------------------------------------------------

const SURFACE_GD: &str = "\
extends Node2D
var elapsed = -1.0
var loops_left = -99
var still_running = false
var interpolated = -1.0

func _ready():
    var t = create_tween()
    t.set_loops(4)
    t.bind_node($A)
    t.set_ignore_time_scale(true)
    t.tween_property($A, \"position:x\", 100.0, 1.0)
    still_running = t.custom_step(0.25)
    elapsed = t.get_total_elapsed_time()
    loops_left = t.get_loops_left()
    interpolated = t.interpolate_value(0.0, 10.0, 0.5, 1.0, Tween.TRANS_LINEAR, Tween.EASE_IN_OUT)
    var f = create_tween()
    f.tween_property($B, \"position:x\", 90.0, 1.0).from(70.0)
";

/// The twelve verbs the audit found as ClassDB `MethodInfo` entries with no
/// dispatch arm: each one raised `UndefinedFunction` when a script called it.
/// This drives the ones that return a value, so a regression to
/// "advertised but not callable" fails here rather than in a game.
#[test]
fn tween_script_surface_is_callable_not_just_advertised() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("surface");
    write_project(&dir, MAIN_TSCN, SURFACE_GD);

    let mut boot = boot(&dir);
    let main = main_id(&boot);
    let tree = boot.main_loop().unwrap().tree();

    // `custom_step(0.25)` advanced a 1s tweener a quarter of the way, so the
    // tween is still running and has banked 0.25s of its own time.
    assert_eq!(
        script_var(tree, main, "still_running"),
        Variant::Bool(true),
        "custom_step() reports the tween is still running"
    );
    match script_var(tree, main, "elapsed") {
        Variant::Float(f) => assert!(
            (f - 0.25).abs() < 1e-6,
            "get_total_elapsed_time() must see custom_step's 0.25s, got {f}"
        ),
        other => panic!("get_total_elapsed_time() should return a float, got {other:?}"),
    }
    assert_eq!(
        as_int(script_var(tree, main, "loops_left")),
        4,
        "get_loops_left() reports the configured loops before any completes"
    );
    match script_var(tree, main, "interpolated") {
        Variant::Float(f) => assert!(
            (f - 5.0).abs() < 1e-6,
            "interpolate_value(0, 10, 0.5, 1.0, LINEAR, IN_OUT) is the midpoint 5.0, got {f}"
        ),
        other => panic!("interpolate_value() should return a float, got {other:?}"),
    }

    // `.from(70.0)` overrides the sampled start: B is at 0 in the scene, so the
    // first frame must land above 70, not just above 0.
    boot.main_loop_mut().unwrap().step(0.1);
    let x = node_x(boot.main_loop().unwrap().tree(), "/root/Main/B");
    assert!(
        x > 70.0 && x < 90.0,
        "from(70.0) starts the tweener at 70 rather than at the node's 0, got {x}"
    );
}
