//! pat-mgixk.10 — the deferred-call queue is flushed after EVERY physics tick,
//! not once per frame.
//!
//! Godot ground truth: `SceneTree` flushes the `MessageQueue` after
//! `_physics_process` as well as after `_process`
//! (upstream/godot/scene/main/scene_tree.cpp:647, :704, :711). Patina flushed
//! only after the idle pass (`main_loop.rs`, the post-process site), so with
//! more than one physics tick in a frame — the normal case at 60 physics ticks
//! against a slower frame rate — a `call_deferred` issued from
//! `_physics_process` was not applied until after *every remaining tick of that
//! frame* had run. A body that deferred its own `queue_free` kept colliding for
//! the rest of the frame, and `get_parent().call_deferred("add_child", n)` from
//! a physics callback could not be observed by the next tick.
//!
//! Entry point, per the bead's wiring requirement — nothing is queued or
//! flushed by hand:
//!
//! ```text
//! project.godot + .tscn + .gd  ->  EngineBootstrap  ->  MainLoop::step
//!   ->  physics tick loop: process_physics_frame_with_scripts (_physics_process
//!       runs `call_deferred`)  ->  process_deferred_calls  ->  next tick
//! ```
//!
//! ⚠️ **Why the deferred target is a SCRIPT method and not a native one.** The
//! native half of this bead (deferred calls reaching native methods) already
//! shipped inside pat-mgixk.9, which removed the
//! `self.scripts.contains_key(&call.node_id)` gate in
//! `SceneTree::process_deferred_calls`. Deferring a native method here would
//! therefore pass with or without the flush change and prove nothing about
//! WHEN the queue runs. A script method isolates the one thing this test is
//! for: the flush *timing*.
//!
//! ⚠️ **What makes it fail without the change.** `second_tick_saw_flag` is
//! written during tick 2 from a flag that tick 1 deferred. Flushed per tick it
//! reads 1; flushed once per frame the deferral is still queued when tick 2
//! runs, so it reads 0. `ticks` is asserted to be exactly 2 first, because if
//! only one physics tick ran the observation would never be made and a `-1`
//! would sail past a laxer assertion.
//!
//! The remaining items of pat-mgixk.10's acceptance — `.bind()` args surviving
//! `Callable.call_deferred`, and lambdas being queueable — need an arm in
//! `gdscript-interop/src/interpreter.rs`, which is under another agent's
//! exclusive lease at the time of writing. They are deliberately NOT asserted
//! here: a knowingly-red test in this crate would break every peer's verify.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `EngineBootstrap` touches process-global engine state, so the boots in this
/// file are serialised. Poison-tolerant: a panic in one test must not cascade
/// into "lock poisoned" failures in the rest.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk10_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")
"#;

/// Tick 1 defers `mark`; tick 2 records whether it has been applied yet.
///
/// Written with plain `if` statements rather than `elif`/ternaries to keep the
/// fixture on syntax the interpreter is already exercised on elsewhere.
const MAIN_GD: &str = r#"extends Node2D

var ticks = 0
var flag = false
var second_tick_saw_flag = -1

func _physics_process(_delta):
    ticks += 1
    if ticks == 1:
        call_deferred("mark")
    if ticks == 2:
        if flag:
            second_tick_saw_flag = 1
        else:
            second_tick_saw_flag = 0

func mark():
    flag = true
"#;

fn write_project(dir: &Path, tscn: &str, gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        // 60 physics ticks/second, so a single 2/60 s frame runs exactly two
        // ticks and the per-tick flush becomes observable.
        "[application]\nconfig/name=\"DeferredFlush\"\nrun/main_scene=\"res://main.tscn\"\n\
         \n[physics]\n\ncommon/physics_ticks_per_second=60\n",
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

fn script_var(tree: &gdscene::SceneTree, node: gdscene::node::NodeId, var: &str) -> Variant {
    tree.get_script(node)
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

fn as_bool(v: Variant) -> bool {
    match v {
        Variant::Bool(b) => b,
        other => panic!("expected a bool, got {other:?}"),
    }
}

#[test]
fn deferred_queue_flushes_between_physics_ticks() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("between_ticks");
    write_project(&dir, MAIN_TSCN, MAIN_GD);

    let mut boot = boot(&dir);
    let main_id = boot
        .main_loop()
        .expect("main loop")
        .tree()
        .get_node_by_path("/root/Main")
        .expect("Main");

    // One frame worth two fixed ticks at 60 Hz.
    boot.main_loop_mut().unwrap().step(2.0 / 60.0);

    let tree = boot.main_loop().unwrap().tree();

    // Premise first: without two ticks in this frame the real assertion below
    // is vacuous, and `second_tick_saw_flag` would still be its `-1` sentinel.
    assert_eq!(
        as_int(script_var(tree, main_id, "ticks")),
        2,
        "the fixture needs exactly two physics ticks in one frame for the \
         flush point to be observable",
    );

    // 🔑 The bead: tick 1's `call_deferred("mark")` must have run before tick 2.
    assert_eq!(
        as_int(script_var(tree, main_id, "second_tick_saw_flag")),
        1,
        "a call_deferred issued from _physics_process must be flushed before \
         the NEXT physics tick (Godot: scene_tree.cpp:647). Reading 0 means the \
         queue was only flushed after the idle pass, so the whole rest of the \
         frame ran against stale state; reading -1 means tick 2 never happened.",
    );

    // And it really did get applied, rather than the observation being skipped.
    assert!(
        as_bool(script_var(tree, main_id, "flag")),
        "the deferred `mark` must actually have run",
    );
}

// ---------------------------------------------------------------------------
// A deferred call reaching a NATIVE method on a scriptless node.
// ---------------------------------------------------------------------------

const FREE_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="Doomed" type="Node2D" parent="."]
"#;

/// `Doomed` carries no script on purpose: the old
/// `SceneTree::process_deferred_calls` gated the whole queue on
/// `self.scripts.contains_key(&call.node_id)`, so a scriptless target was
/// dropped before any method lookup happened.
const FREE_GD: &str = r#"extends Node2D

var deferred = false

func _ready():
    $Doomed.call_deferred("queue_free")
    deferred = true
"#;

/// `$Doomed.call_deferred("queue_free")` actually frees the node.
///
/// `queue_free` is a NATIVE method (`SceneAccess::call_node_method`'s
/// `"queue_free"` arm → `SceneTree::queue_free`), and `Doomed` has no script,
/// so this is the exact shape the old gate discarded. `MainLoop::step` runs
/// `process_deferred_calls` and then `process_deletions` in that order within
/// one frame, so a single step is enough for the free to be observable.
///
/// ⚠️ This covers the native-dispatch half of pat-mgixk.10, which landed inside
/// pat-mgixk.9 — it is NOT asserting the flush-timing change the first test
/// covers. Both belong to this bead's acceptance, so both are asserted here
/// rather than taking the other bead's word for it.
#[test]
fn call_deferred_reaches_a_native_method_on_a_scriptless_node() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("native_free");
    write_project(&dir, FREE_TSCN, FREE_GD);

    let mut boot = boot(&dir);

    // Premise: `_ready` ran and enqueued the deferred call, and the node is
    // still here. Without this a missing node below could just mean the scene
    // never built it.
    {
        let tree = boot.main_loop().expect("main loop").tree();
        let main_id = tree.get_node_by_path("/root/Main").expect("Main");
        assert!(
            as_bool(script_var(tree, main_id, "deferred")),
            "_ready must have run and issued the deferred queue_free",
        );
        assert!(
            tree.get_node_by_path("/root/Main/Doomed").is_some(),
            "Doomed must exist before the queue is flushed",
        );
    }

    boot.main_loop_mut().unwrap().step(1.0 / 60.0);

    assert!(
        boot.main_loop()
            .unwrap()
            .tree()
            .get_node_by_path("/root/Main/Doomed")
            .is_none(),
        "a deferred `queue_free` on a SCRIPTLESS node must actually free it —          if Doomed survives, the deferred queue dropped the call because the          target had no script instance",
    );
}

// ---------------------------------------------------------------------------
// The bead's headline idiom: a deferred `add_child`.
// ---------------------------------------------------------------------------

const ADD_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="Holder" type="Node2D" parent="."]
"#;

/// `Node2D.new()` yields a DETACHED node (`create_node`, no parent), which is
/// the shape `SceneAccess::add_child` routes through `take_node` + `add_child`.
const ADD_GD: &str = r#"extends Node2D

var queued = false

func _ready():
    var n = Node2D.new()
    $Holder.call_deferred("add_child", n)
    queued = true
"#;

fn child_count(tree: &gdscene::SceneTree, node: gdscene::node::NodeId) -> usize {
    tree.get_node(node)
        .expect("node exists")
        .children()
        .len()
}

/// `$Holder.call_deferred("add_child", Node2D.new())` actually parents the node.
///
/// ⚠️ This is the idiom the bead is named for, and it needed BOTH halves:
/// pat-mgixk.9 removing the queue's `scripts.contains_key` gate, and an
/// `"add_child"` arm in `SceneAccess::call_node_method` (added for
/// pat-mgixk.10). The gate alone was not enough — the direct spellings
/// `add_child(n)` and `$X.add_child(n)` call `SceneAccess::add_child`
/// themselves, so only the DEFERRED route came through the dispatch table, and
/// that table had no `add_child`. The call returned the silent `Variant::Nil`
/// placeholder and the node was never parented.
#[test]
fn call_deferred_add_child_parents_the_node() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("add_child");
    write_project(&dir, ADD_TSCN, ADD_GD);

    let mut boot = boot(&dir);
    let (main_id, holder_id) = {
        let tree = boot.main_loop().expect("main loop").tree();
        (
            tree.get_node_by_path("/root/Main").expect("Main"),
            tree.get_node_by_path("/root/Main/Holder").expect("Holder"),
        )
    };

    // Premise: `_ready` ran and queued the call, and Holder is still empty —
    // so a child appearing below cannot be something the scene itself built.
    {
        let tree = boot.main_loop().unwrap().tree();
        assert!(
            as_bool(script_var(tree, main_id, "queued")),
            "_ready must have run and issued the deferred add_child",
        );
        assert_eq!(
            child_count(tree, holder_id),
            0,
            "Holder must start empty, before the queue is flushed",
        );
    }

    boot.main_loop_mut().unwrap().step(1.0 / 60.0);

    assert_eq!(
        child_count(boot.main_loop().unwrap().tree(), holder_id),
        1,
        "a deferred `add_child` must actually parent the node — a count of 0          means the deferred call reached `call_node_method` and found no          `add_child` arm, so it returned the silent Nil placeholder",
    );
}
