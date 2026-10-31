
//! pat-mgixk.9 part 2: script dispatch is **re-entrant** — a signal or call
//! that comes back to a script which is already running is delivered, in the
//! same frame.
//!
//! ## What was broken
//!
//! `SceneTree::call_script_method` takes the script out of `SceneTree.scripts`
//! for the duration of its call, so a nested dispatch back to it found nothing:
//!
//! * `emit_signal` → `call_object_method` → `call_script_method` → arena miss →
//!   the handler was dropped, silently.
//! * `node_has_method` answered **false** for a running script, so
//!   `Interpreter::call_object_method` raised `UndefinedFunction` on a
//!   legitimate callback into it.
//!
//! Godot has no such window: `Object::callp` and `emit_signalp` are fully
//! re-entrant. The engine admitted the gap in its own comments — see
//! `signal_value_emit_connect.rs:73-77` ("a signal emitted during
//! Main._ready() ... cannot be delivered back to Main's own handler") and
//! `scene_tree.rs:952-956` — and `scripting.rs` defers an RPC `call_local`
//! specifically to dodge it.
//!
//! The canonical breakage is the one this test drives: `Player` calls
//! `enemy.take_damage()`, `Enemy` emits `died`, and `Player._on_enemy_died`
//! never ran.
//!
//! ## The three routes, which need three different things to work
//!
//! 1. **A → B → A by signal.** `Player._process` calls `$Enemy.take_damage()`;
//!    Enemy emits `died`; Player's handler must run. Needs `emit_signal` to
//!    reach a checked-out script.
//! 2. **A → A by signal.** Player emits its own `self_ping`. Same path, but the
//!    emitter and the target are the same checked-out script.
//! 3. **A → B → A by direct call, returning a value.** `$Enemy.call_back_owner()`
//!    does `get_parent().note()`. This one additionally needs
//!    `node_has_method` to stop lying, *and* the value to come back — which is
//!    why deferring re-entrant handlers to the end of the frame (the cheap fix)
//!    cannot satisfy it: a deferred call has nobody to return to.
//!
//! Entry point is the real runner: `patina_runner::bootstrap` loads
//! `project.godot` + `main.tscn` from disk and `MainLoop::step` drives one
//! frame, so every route is exercised the way a game would.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `EngineBootstrap` touches process-global state (ClassDB, resource roots), so
/// the runner tests serialize on one lock.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

const DT: f64 = 1.0 / 60.0;

/// What `Player.note()` returns, so route 3 proves a *value* came back rather
/// than just that nothing raised. Deliberately not 0 or 1: the vars it is
/// compared against start at 0, and a dropped call would leave it there.
const NOTE_VALUE: i64 = 7;

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk9_reentrant_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `Player` is the scene root so `Enemy.get_parent()` is Player — route 3 then
/// needs no handle passed through the call.
const MAIN_TSCN: &str = r#"[gd_scene load_steps=3 format=3]

[ext_resource type="Script" path="res://player.gd" id="1_player"]
[ext_resource type="Script" path="res://enemy.gd" id="2_enemy"]

[node name="Player" type="Node2D"]
script = ExtResource("1_player")

[node name="Enemy" type="Node2D" parent="."]
script = ExtResource("2_enemy")
"#;

/// ⚠️ `stage` is bumped after every statement in `_process`, so an abort names
/// the exact call that failed. Without it, all three routes read as "the
/// counter is 0" and a raise in the first statement is indistinguishable from a
/// dropped handler in the third.
const PLAYER_GD: &str = "\
extends Node2D
signal self_ping
var stage = 0
var connect_died = -1
var connect_ping = -1
var died_count = 0
var ping_count = 0
var callback_value = 0
var ran = false

func _ready():
    connect_died = $Enemy.connect(\"died\", Callable(self, \"_on_enemy_died\"))
    connect_ping = connect(\"self_ping\", Callable(self, \"_on_self_ping\"))

func _process(delta):
    if ran:
        return
    ran = true
    stage = 1
    $Enemy.take_damage(10)
    stage = 2
    emit_signal(\"self_ping\")
    stage = 3
    var e = get_node(\"Enemy\")
    callback_value = e.call_back_owner(self)
    stage = 4

func _on_enemy_died():
    died_count = died_count + 1

func _on_self_ping():
    ping_count = ping_count + 1

func note():
    return 7
";

/// Enemy emits `died` from inside a call that Player made, which is the whole
/// point: Player is checked out of the arena at that moment.
const ENEMY_GD: &str = "\
extends Node2D
signal died
var hp = 10
var emitted = false

func take_damage(amount):
    hp = hp - amount
    if hp <= 0:
        emitted = true
        emit_signal(\"died\")

func call_back_owner(owner_node):
    return owner_node.note()
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"Reentrant\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("player.gd"), PLAYER_GD).unwrap();
    std::fs::write(dir.join("enemy.gd"), ENEMY_GD).unwrap();
}

fn boot(dir: &Path) -> EngineBootstrap {
    gdobject::class_db::register_2d_classes();
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

/// **The acceptance test.**
#[test]
fn script_reentrant_signal_dispatch() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("main");
    write_project(&dir);

    let mut boot = boot(&dir);
    // A few frames, not one. `_ready` has to run before the `connect` calls
    // mean anything, and this does not depend on whether the runner delivers
    // `_ready` and the first `_process` in the same step. The script's own
    // `ran` guard keeps the body to a single execution, so the counts below
    // still assert exactly 1 and would catch a handler firing twice.
    for _ in 0..3 {
        boot.main_loop_mut().expect("main loop").step(DT);
    }

    let tree = boot.main_loop().expect("main loop built").tree();
    let player = tree
        .get_node_by_path("/root/Player")
        .expect("Player is the scene root");
    let script = tree.get_script(player).expect("Player has a script");
    let var = |name: &str| script.get_property(name);

    // -- 0. did `_process` finish? -------------------------------------------
    //
    // Checked first: a raise aborts the whole handler, so every count below
    // would read 0 and look like three dropped dispatches instead of one
    // runtime error.
    let stage = match var("stage") {
        Some(Variant::Int(i)) => i,
        other => panic!("`stage` should be an Int, got {other:?}"),
    };
    assert_eq!(
        var("ran"),
        Some(Variant::Bool(true)),
        "Player._process must have run at all; a scene where it never fired \
         would satisfy nothing below by accident"
    );
    assert_eq!(
        stage, 4,
        "`_process` aborted at stage {stage}: 1 = $Enemy.take_damage(10) \
         raised, 2 = emit_signal(\"self_ping\") raised, 3 = \
         $Enemy.call_back_owner() raised (that one is the `node_has_method` \
         lie — UndefinedFunction on a callback into a running script)"
    );

    // -- 1. the fixture links, asserted BEFORE the routes that depend on them
    //
    // ⚠️ Ordered deliberately. The first version of this test asserted
    // `died_count == 1` straight after the stage check and failed with
    // `Some(Int(0))` — which is equally consistent with "the connection was
    // never made", "`take_damage` never reached the emit", and "the re-entrant
    // dispatch dropped the handler". Each link now fails on its own line.
    let enemy = tree
        .get_node_by_path("/root/Player/Enemy")
        .expect("Enemy must still be in the tree");
    let enemy_script = tree.get_script(enemy).expect("Enemy has a script");

    assert_eq!(
        enemy_script.get_property("hp"),
        Some(Variant::Int(0)),
        "link 1: $Enemy.take_damage(10) must run Enemy's script method and take \
         hp from 10 to 0"
    );
    assert_eq!(
        enemy_script.get_property("emitted"),
        Some(Variant::Bool(true)),
        "link 2: hp <= 0, so take_damage must have REACHED emit_signal(\"died\"). \
         False means the emit never happened and route 1 below could not fire \
         for a reason that has nothing to do with re-entrancy"
    );
    // `connect` returns a Godot `Error`; what matters here is only that the
    // call dispatched at all, so this checks it is no longer the script's
    // sentinel rather than asserting a specific status code.
    assert_ne!(
        var("connect_died"),
        Some(Variant::Int(-1)),
        "link 3: $Enemy.connect(\"died\", Callable(self, \"_on_enemy_died\")) must \
         dispatch. Still -1 means the connect itself never ran, so no \
         connection exists to deliver"
    );
    assert_ne!(
        var("connect_ping"),
        Some(Variant::Int(-1)),
        "link 4: the self-connect for `self_ping` must dispatch too"
    );

    // -- 2. 🔑 A → B → A by signal -------------------------------------------
    assert_eq!(
        var("died_count"),
        Some(Variant::Int(1)),
        "Player called $Enemy.take_damage(10), Enemy emitted `died`, and \
         Player._on_enemy_died must have run — in the same frame. 0 means the \
         handler was dropped because Player was checked out of the arena, \
         which is the canonical breakage this bead names"
    );

    // -- 2. A → A by signal --------------------------------------------------
    assert_eq!(
        var("ping_count"),
        Some(Variant::Int(1)),
        "Player emitted its own `self_ping` from _process, connected to its own \
         handler; emitter and target are the same checked-out script"
    );

    // -- 3. 🔑 A → B → A by direct call, with a RETURN VALUE -----------------
    //
    // This is the clause that rules out the cheap fix. Deferring a re-entrant
    // handler to the end of the frame delivers routes 1 and 2 but has nobody
    // to return a value to, so it cannot satisfy this one.
    assert_eq!(
        var("callback_value"),
        Some(Variant::Int(NOTE_VALUE)),
        "$Enemy.call_back_owner() does get_parent().note(), and Player.note() \
         returns {NOTE_VALUE}. 0 means the call never reached Player's running \
         script and the assignment kept its initial value"
    );
}
