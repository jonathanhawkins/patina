//! pat-mgixk.59 acceptance: `get_tree()`, `get_viewport()` and `get_window()`
//! are real objects a script can store, read properties off, assign to, await
//! signals on and call methods on.
//!
//! Godot ground truth: `scene/main/node.h` `get_tree`/`get_viewport`/
//! `get_window`; `scene/main/scene_tree.cpp` `_bind_methods` (`paused`,
//! `current_scene`, `root`, and the `process_frame`/`physics_frame`/`node_added`
//! signals); `scene/main/viewport.cpp` `get_visible_rect`/
//! `set_input_as_handled`.
//!
//! # What was broken
//!
//! `get_tree` was handled **syntactically and only syntactically**: the
//! interpreter matched the literal call form `get_tree().<method>(...)` and
//! dispatched that one shape. `get_tree` itself never evaluated, so every one of
//! these raised `UndefinedFunction`:
//!
//! * `var t = get_tree()` — the object as a value
//! * `get_tree().paused = true` — the pause-menu idiom
//! * `get_tree().current_scene`, `get_tree().root` — property reads
//! * `await get_tree().process_frame` — parse-tested only, never run
//!
//! and `get_viewport` / `get_window` had no handling at all (0 hits across every
//! crate), so `get_viewport().set_input_as_handled()` and
//! `get_viewport().get_visible_rect()` failed too.
//!
//! 🔑 Every assertion below is on **engine state or script-visible values after
//! real frames**, never on "the call did not raise". A test that only proved the
//! calls resolve would pass against a `get_tree()` that returned an inert stub:
//! the pause has to actually stop a `_process`, the await has to actually
//! resume, and the visible rect has to be the base size rather than the
//! window's.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdcore::math::Vector2;
use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `boot_project` mutates the process-global `res://` path roots, so serialize
/// these tests to stay correct under `cargo test` (one process, many threads).
/// `cargo nextest` isolates each test in its own process and does not need this.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

const DT: f64 = 1.0 / 60.0;

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk59_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes a bootable project. `display` is spliced into `project.godot` so one
/// test can configure the stretch settings without affecting the others.
fn write_project(dir: &Path, gd: &str, display: &str) {
    std::fs::write(
        dir.join("project.godot"),
        format!(
            "config_version=5\n\n[application]\n\nrun/main_scene=\"res://main.tscn\"\n{display}"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1_main\"]\n\n\
         [node name=\"Main\" type=\"Node2D\"]\nscript = ExtResource(\"1_main\")\n",
    )
    .unwrap();
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

fn script_var(boot: &EngineBootstrap, var: &str) -> Variant {
    let tree = boot.main_loop().unwrap().tree();
    let id = tree
        .get_node_by_path("/root/Main")
        .expect("the main scene root is in the tree");
    tree.get_script(id)
        .expect("Main has a script")
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
        .unwrap_or(Variant::Nil)
}

fn int_var(boot: &EngineBootstrap, var: &str) -> i64 {
    match script_var(boot, var) {
        Variant::Int(n) => n,
        other => panic!("`{var}` should be an Int, got {other:?}"),
    }
}

fn string_var(boot: &EngineBootstrap, var: &str) -> String {
    match script_var(boot, var) {
        Variant::String(s) => s,
        other => panic!("`{var}` should be a String, got {other:?}"),
    }
}

/// Stores the tree in a local, reads two of its properties, awaits one of its
/// signals, and finally pauses it through the property.
///
/// ⚠️ `current_scene` and `root` are read in `_process`, not `_ready`: the
/// runner assigns `current_scene` as part of instancing the main scene, and
/// reading it from `_ready` would race that assignment and fail for a reason
/// unrelated to this bead.
const LIVE_TREE_GD: &str = "\
extends Node2D
var ticks = 0
var awaited = false
var scene_name = \"<unset>\"
var root_name = \"<unset>\"

func _ready():
    await get_tree().process_frame
    self.awaited = true

func _process(delta):
    self.ticks = self.ticks + 1
    if self.ticks == 1:
        self.scene_name = get_tree().current_scene.name
        self.root_name = get_tree().root.name
    if self.ticks == 2:
        var t = get_tree()
        t.paused = true
";

/// **The acceptance test.** The SceneTree is a live object: its properties read,
/// its `paused` assignment takes effect on the running loop, its signals are
/// awaitable, and it survives being stored in a variable.
#[test]
fn get_tree_and_get_viewport_are_live_objects() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("live_tree");
    write_project(&dir, LIVE_TREE_GD, "");
    let mut boot = boot(&dir);

    // CONTROL: the tree starts unpaused, so "paused" below is a change this
    // script caused rather than the state it booted into.
    assert!(
        !boot.main_loop().unwrap().tree().paused(),
        "precondition: a freshly booted tree is not paused"
    );

    for _ in 0..5 {
        boot.main_loop_mut().unwrap().step(DT);
    }

    // -- the tree really stopped processing ---------------------------------
    //
    // 🔑 The counter froze at 2 across FIVE frames. This is the assertion that
    // distinguishes a working `paused` write from one that silently landed in a
    // property bag: an inert write leaves `_process` running and this reads 5.
    assert_eq!(
        int_var(&boot, "ticks"),
        2,
        "`get_tree().paused = true` on tick 2 must stop this node's `_process` \
         for the remaining frames; 5 means the write never reached the tree"
    );
    assert!(
        boot.main_loop().unwrap().tree().paused(),
        "...and the engine's own pause flag must be set, not just the script's \
         view of it"
    );

    // -- property reads -----------------------------------------------------
    assert_eq!(
        string_var(&boot, "scene_name"),
        "Main",
        "`get_tree().current_scene` must be the main scene's root node"
    );
    assert_eq!(
        string_var(&boot, "root_name"),
        "root",
        "`get_tree().root` must be the tree root Window"
    );

    // -- `await get_tree().process_frame` resumed ---------------------------
    //
    // `MainLoop` emits `process_frame` on the tree ROOT, so this only resumes if
    // the Signal handed out by `get_tree().process_frame` names the root. A
    // Signal bound to anything else parks the coroutine forever and this stays
    // false — exactly the failure a plausible-looking stub would have.
    assert_eq!(
        script_var(&boot, "awaited"),
        Variant::Bool(true),
        "`await get_tree().process_frame` must resume on a later frame"
    );
}

/// `get_viewport().get_visible_rect()` is the project's BASE resolution, not the
/// window size.
const VISIBLE_RECT_GD: &str = "\
extends Node2D
var w = -1.0
var h = -1.0

func _process(delta):
    var r = get_viewport().get_visible_rect()
    self.w = r.size.x
    self.h = r.size.y
";

/// 640x360 base inside a 1280x720 window.
///
/// 🔑 The window is resized AFTER boot so the two numbers genuinely differ.
/// `project.godot` seeds both the base size and the window size from
/// `viewport_width`/`viewport_height`, so without the resize they would be equal
/// and "the visible rect is the base size" could not be told apart from "the
/// visible rect is the window size" — the one mistake this clause exists to
/// catch. `viewport.rs` states the rule directly: a 640x360 base in a larger
/// window still reports 640x360.
#[test]
fn get_visible_rect_reports_the_stretch_base_size_not_the_window() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("visible_rect");
    write_project(
        &dir,
        VISIBLE_RECT_GD,
        "\n[display]\n\n\
         window/size/viewport_width=640\n\
         window/size/viewport_height=360\n\
         window/stretch/mode=\"canvas_items\"\n",
    );
    let mut boot = boot(&dir);

    boot.main_loop_mut()
        .unwrap()
        .tree_mut()
        .root_window_mut()
        .size = Vector2::new(1280.0, 720.0);

    // CONTROL: the window and the base size now differ, so the assertion below
    // discriminates between them.
    assert_eq!(
        boot.main_loop().unwrap().tree().root_window().size,
        Vector2::new(1280.0, 720.0),
        "precondition: the window is twice the base size"
    );

    boot.main_loop_mut().unwrap().step(DT);

    let w = match script_var(&boot, "w") {
        Variant::Float(f) => f,
        other => panic!("`w` should be a Float, got {other:?}"),
    };
    let h = match script_var(&boot, "h") {
        Variant::Float(f) => f,
        other => panic!("`h` should be a Float, got {other:?}"),
    };
    assert_eq!(
        (w, h),
        (640.0, 360.0),
        "`get_viewport().get_visible_rect().size` must be the project's base \
         resolution; (1280, 720) would mean it reported the WINDOW, and \
         (-1, -1) that the call never resolved"
    );
}

const HANDLES_INPUT_GD: &str = "\
extends Node2D
var seen_input = 0
var seen_unhandled = 0

func _input(event):
    self.seen_input = self.seen_input + 1
    get_viewport().set_input_as_handled()

func _unhandled_input(event):
    self.seen_unhandled = self.seen_unhandled + 1
";

/// The CONTROL script: identical except it never marks the event handled.
const PASSES_INPUT_GD: &str = "\
extends Node2D
var seen_input = 0
var seen_unhandled = 0

func _input(event):
    self.seen_input = self.seen_input + 1

func _unhandled_input(event):
    self.seen_unhandled = self.seen_unhandled + 1
";

/// `get_viewport().set_input_as_handled()` in `_input` stops `_unhandled_input`
/// for that event.
///
/// ⚠️ Run against BOTH scripts. `seen_unhandled == 0` on its own is also what a
/// fixture that never dispatched `_unhandled_input` at all would report, so the
/// control — same event, same stages, no `set_input_as_handled()` — is what
/// makes the zero mean something.
#[test]
fn viewport_set_input_as_handled_stops_the_unhandled_stage() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let mut results = Vec::new();
    for (tag, gd) in [("handled", HANDLES_INPUT_GD), ("control", PASSES_INPUT_GD)] {
        let dir = unique_project_dir(tag);
        write_project(&dir, gd, "");
        let mut boot = boot(&dir);
        boot.main_loop_mut()
            .unwrap()
            .push_event(gdplatform::input::InputEvent::Key {
                key: gdplatform::input::Key::A,
                pressed: true,
                shift: false,
                ctrl: false,
                alt: false,
            });
        boot.main_loop_mut().unwrap().step(DT);
        results.push((
            int_var(&boot, "seen_input"),
            int_var(&boot, "seen_unhandled"),
        ));
        let _ = tag;
    }

    let (handled_input, handled_unhandled) = results[0];
    let (control_input, control_unhandled) = results[1];

    // CONTROL FIRST: both scripts saw `_input`, and the control's event reached
    // `_unhandled_input`. Without this pair, the handled case proves nothing.
    assert_eq!(
        control_input, 1,
        "precondition: the control script must receive `_input` once"
    );
    assert_eq!(
        control_unhandled, 1,
        "precondition: an unconsumed key event must reach `_unhandled_input`; 0 \
         would mean the fixture never dispatches that stage and the assertion \
         below is vacuous"
    );
    assert_eq!(
        handled_input, 1,
        "the handling script must still receive `_input` itself"
    );
    assert_eq!(
        handled_unhandled, 0,
        "`get_viewport().set_input_as_handled()` in `_input` must stop \
         `_unhandled_input` for that event; 1 means the call did not reach the \
         viewport's handled flag"
    );
}
