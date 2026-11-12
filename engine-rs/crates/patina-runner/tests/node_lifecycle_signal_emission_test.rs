//! pat-mgixk.64 — the `Node` lifecycle signals, emitted and observable from a
//! script.
//!
//! All nine are registered in the ClassDB (`class_db.rs:1239-1250`), so
//! `connect()` has always succeeded. Only `tree_entered` was ever emitted, so
//! `ready`, `tree_exiting`, `tree_exited`, `child_entered_tree`,
//! `child_exiting_tree`, `child_order_changed`, `renamed` and `replacing_by`
//! were silently dead: a handler connected, never fired, and nothing reported
//! an error.
//!
//! # How these tests record
//!
//! 🔑 Every handler writes to **node metadata on a `Flag` node**, never to a
//! script variable, and that is not a stylistic choice — it is forced.
//!
//! ⚠️ **A signal handler's writes to `self.*` do not survive the handler.**
//! Measured directly: a handler doing `self.hits += 1` prints `1` on the next
//! line *inside* the handler, and the caller then reads `0`. These emissions
//! happen inside a script frame (the driver's `_physics_process` calls
//! `remove_child`, which emits), so the driver's instance is already checked out
//! by the running interpreter; the handler mutates a different copy and the
//! outer frame's stale state is written back over it on return. That is a
//! separate engine defect — reported, not fixed here — and it means a test
//! recording into `self.log` would read an empty log however correct the
//! emissions were. Node metadata lives in the tree, so it persists.
//!
//! ⚠️ **A connected handler plus a property-spelling rename aborts the frame.**
//! Measured: with a handler on `renamed`, `node.name = "x"` leaves the next
//! statement unreached and the node unrenamed; with no handler the rename lands
//! and both signals appear in the trace. `set_name()` — the METHOD spelling —
//! delivers to a handler without trouble, so the fault is in re-entering a
//! script during a property ASSIGNMENT. Reported separately; the rename test
//! below reads the event trace rather than building on it.
//!
//! ⚠️ **`child_entered_tree`, `child_exiting_tree` and `replacing_by` handlers
//! must take ONE parameter.** Godot passes the node. A zero-arity handler is
//! simply not invoked — no error, no record. Measured: with
//! `func on_child_exiting():` the signal was emitted (it shows in the event
//! trace) and the handler never ran; with `func on_child_exiting(n):` it ran and
//! `n.get_name()` was `"Target"`.
//!
//! # Two spellings the bead's acceptance text asks for and these avoid
//!
//! ⚠️ `await child.ready` would **hang, not fail**. `ready` is emitted inside
//! `add_child`, so by the time a later line awaits it the signal has already
//! passed and the coroutine parks forever. A connected handler observes the same
//! emission with no timeout.
//!
//! ⚠️ No `.new()`. It is proven only for `AStar2D`; whether `Node2D.new()`
//! yields a tree-insertable node is unverified. Authored children reach every
//! signal, so it is not needed.
//!
//! # Why `replacing_by` and the `name =` spelling are separate tests
//!
//! ⚠️ `replace_by` **removes** the outgoing node, so it emits the whole exit
//! trio again. Folding it into the main fixture would push `tree_exiting` and
//! `tree_exited` to two occurrences and make "exactly once" unassertable. The
//! `name =` spelling is split out for a different reason: the main test pins an
//! exact emission ORDER that was measured with `set_name`, and a strict
//! whole-sequence assertion is the wrong place to introduce an unmeasured
//! variable.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdscene::trace::TraceEventType;
use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `boot_project` mutates the process-global `res://` roots, so serialize under
/// `cargo test` (one process, many threads). `nextest` isolates per process.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

const DT: f64 = 1.0 / 60.0;

/// Takes the boot lock, tolerating a previous test's panic.
///
/// ⚠️ `lock().unwrap()` turns ONE failing test into three: the panic poisons the
/// mutex and every later test in the process dies with `PoisonError` instead of
/// its own assertion. `nextest` isolates per process and would not show it, but
/// `cargo test` does, and a cascade hides which test actually broke.
fn boot_lock() -> std::sync::MutexGuard<'static, ()> {
    BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk64_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `Main` holds the driver, the node whose lifecycle is watched, a recorder and
/// a replacement. Every one is **authored**, so no node is constructed from a
/// script.
fn write_project(dir: &Path, gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://driver.gd\" id=\"1_drv\"]\n\n\
         [node name=\"Main\" type=\"Node2D\"]\n\n\
         [node name=\"Driver\" type=\"Node2D\" parent=\".\"]\n\
         script = ExtResource(\"1_drv\")\n\n\
         [node name=\"Target\" type=\"Node2D\" parent=\".\"]\n\n\
         [node name=\"Spare\" type=\"Node2D\" parent=\".\"]\n\n\
         [node name=\"Flag\" type=\"Node2D\" parent=\".\"]\n",
    )
    .unwrap();
    std::fs::write(dir.join("driver.gd"), gd).unwrap();
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

fn flag_node(boot: &EngineBootstrap) -> gdscene::node::NodeId {
    boot.main_loop()
        .unwrap()
        .tree()
        .get_node_by_path("/root/Main/Flag")
        .expect("the fixture declares a Flag node")
}

/// Reads a recorded count off the `Flag` node.
fn count(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> i64 {
    match boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node(flag)
        .expect("the Flag node is still in the tree")
        .get_meta(key)
    {
        Variant::Int(n) => n,
        other => panic!("`{key}` must be recorded as an Int, got {other:?}"),
    }
}

/// The shared prologue: handlers that record, and a frame-0 phase that zeroes
/// the counters and connects.
///
/// ⚠️ The driver is a CHILD of `Main`, not `Main` itself, and connects
/// cross-node. `self.connect(..)` is not a dispatched spelling: `connect` has no
/// arm on the self path, and `call_object_method` short-circuits to that path
/// when the target is the current node, so a dot call on `self` raises
/// `UndefinedFunction` and aborts the frame.
///
/// ⚠️ Connections are made on frame 0 and the operations run on later frames, so
/// every listener exists before anything it watches happens.
const PROLOGUE: &str = "\
extends Node2D
var phase = 0
var held = null

func mark(k):
    var f = get_node(\"../Flag\")
    f.set_meta(k, f.get_meta(k) + 1)
    f.set_meta(\"seq\", f.get_meta(\"seq\") + k + \"|\")

func on_ready():
    mark(\"ready\")

func on_renamed():
    mark(\"renamed\")

func on_exiting():
    mark(\"tree_exiting\")

func on_exited():
    mark(\"tree_exited\")

func on_order():
    mark(\"child_order_changed\")

func on_child_entered(n):
    mark(\"child_entered_tree\")

func on_child_exiting(n):
    mark(\"child_exiting_tree\")

func on_replacing(n):
    mark(\"replacing_by\")
    if self.held.get_parent() != null:
        get_node(\"../Flag\").set_meta(\"still_parented\", 1)

func connect_all():
    var f = get_node(\"../Flag\")
    f.set_meta(\"seq\", \"\")
    for k in [\"ready\", \"renamed\", \"tree_exiting\", \"tree_exited\", \"child_order_changed\", \"child_entered_tree\", \"child_exiting_tree\", \"replacing_by\", \"still_parented\"]:
        f.set_meta(k, 0)
    var t = get_node(\"../Target\")
    self.held = t
    t.connect(\"ready\", on_ready)
    t.connect(\"renamed\", on_renamed)
    t.connect(\"tree_exiting\", on_exiting)
    t.connect(\"tree_exited\", on_exited)
    t.connect(\"replacing_by\", on_replacing)
    get_parent().connect(\"child_entered_tree\", on_child_entered)
    get_parent().connect(\"child_exiting_tree\", on_child_exiting)
    get_parent().connect(\"child_order_changed\", on_order)
";

/// Reorder, rename, remove, re-add — the operation list the bead's acceptance
/// describes, each reaching a different signal.
const GD_LIFECYCLE: &str = "
func _physics_process(delta):
    if self.phase == 0:
        self.phase = 1
        connect_all()
        return
    if self.phase == 1:
        self.phase = 2
        get_parent().move_child(self.held, 0)
        self.held.set_name(\"Renamed\")
        get_parent().remove_child(self.held)
        return
    if self.phase == 2:
        self.phase = 3
        get_parent().add_child(self.held)
        return
";

/// **The bead's named acceptance test.** Seven lifecycle signals, each recorded
/// by a connected GDScript handler, driven through the real frame loop.
#[test]
fn node_lifecycle_signal_emission_test() {
    let _guard = boot_lock();
    let dir = unique_project_dir("lifecycle");
    write_project(&dir, &format!("{PROLOGUE}{GD_LIFECYCLE}"));
    let mut boot = boot(&dir);
    let flag = flag_node(&boot);

    // Frame 0 connects, 1 moves/renames/removes, 2 re-adds; one more lets any
    // deferred work land.
    for _ in 0..4 {
        boot.main_loop_mut().unwrap().step(DT);
    }

    // -- the once-per-operation signals --------------------------------------
    for key in [
        "ready",
        "renamed",
        "tree_exiting",
        "tree_exited",
        "child_entered_tree",
        "child_exiting_tree",
    ] {
        assert_eq!(
            count(&boot, flag, key),
            1,
            "`{key}` must reach its connected handler exactly once; 0 means it \
             was never emitted — or emitted and never delivered — which is what \
             a ClassDB-registered signal that nothing fires looks like"
        );
    }

    // -- `child_order_changed` fires per reorder, and there are three ---------
    //
    // ⚠️ Three, not one: `move_child`, `remove_child` and `add_child` each
    // change the child order, and Godot emits on all three. Asserting 1 here
    // would be asserting that two of the emission sites are missing — the exact
    // bug of wiring only some of them.
    assert_eq!(
        count(&boot, flag, "child_order_changed"),
        3,
        "`child_order_changed` must fire for each of move_child, remove_child \
         and add_child"
    );

    // -- Godot's ordering ----------------------------------------------------
    //
    // 🔑 The two orderings the bead names are both in here and neither is
    // implied by the counts above: `tree_exiting` comes BEFORE the node leaves
    // (so a handler can still read its parent), and `tree_exited` comes AFTER
    // detachment. An implementation emitting both from one place would satisfy
    // every count assertion above.
    let seq = match boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node(flag)
        .unwrap()
        .get_meta("seq")
    {
        Variant::String(s) => s,
        other => panic!("the sequence must be recorded as a String, got {other:?}"),
    };
    assert_eq!(
        seq,
        "child_order_changed|renamed|tree_exiting|child_exiting_tree|\
         child_order_changed|tree_exited|child_order_changed|\
         child_entered_tree|ready|",
        "the emission ORDER must match Godot's: the reorder, the rename, then \
         the exit trio with `tree_exiting` before and `tree_exited` after \
         detachment, then the re-entry pair"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `node.name = "x"` — Godot's property spelling for a rename.
///
/// ⚠️ Measured at HEAD, this was a complete **no-op**: the generic property
/// writer stored a `name` entry in the node's bag, which nothing reads
/// (`Node::name` is a struct field, not a bag entry), so `get_name()` still
/// answered the old name and `renamed` never fired — while the `set_name()`
/// METHOD spelling worked. Two spellings of one operation disagreed, and the
/// property one is the spelling every Godot tutorial uses.
///
/// 🔑 Both halves are asserted. A fix that emitted `renamed` without actually
/// renaming, or renamed without emitting, passes exactly one of them.
///
/// ⚠️ This fixture connects NOTHING. See the test body: a connected `renamed`
/// handler plus a property-spelling rename aborts the frame, so the emission is
/// read out of the event trace instead.
const GD_NAME_PROP: &str = "
func _physics_process(delta):
    if self.phase == 0:
        self.phase = 1
        get_node(\"../Target\").name = \"ByProperty\"
        return
";

#[test]
fn name_property_assignment_renames_and_emits_renamed() {
    let _guard = boot_lock();
    let dir = unique_project_dir("nameprop");
    write_project(&dir, &format!("{PROLOGUE}{GD_NAME_PROP}"));
    let mut boot = boot(&dir);

    // 🔑 Observed through the EVENT TRACE, with no handler connected, and that
    // is forced rather than preferred.
    //
    // ⚠️ Connecting a GDScript handler to `renamed` and then renaming through
    // the PROPERTY spelling **aborts the frame**. Measured: with a handler
    // attached the statement after the assignment never runs and the node is
    // not renamed; with no handler the rename lands and both `node_renamed` and
    // `renamed` appear in the trace. The method spelling `set_name()` delivers
    // to a handler fine, so the fault is in re-entering a script during a
    // property ASSIGNMENT, not in the emission — reported separately, and not
    // something this test should be built on top of.
    boot.main_loop_mut()
        .unwrap()
        .tree_mut()
        .event_trace_mut()
        .enable();

    for _ in 0..3 {
        boot.main_loop_mut().unwrap().step(DT);
    }

    assert!(
        boot.main_loop()
            .unwrap()
            .tree()
            .get_node_by_path("/root/Main/ByProperty")
            .is_some(),
        "`name = \"ByProperty\"` must actually rename the node; still finding it \
         under its old path means the assignment landed in the property bag, \
         which nothing reads"
    );

    let renamed_events: Vec<&str> = boot
        .main_loop()
        .unwrap()
        .tree()
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::SignalEmit && e.detail == "renamed")
        .map(|e| e.node_path.as_str())
        .collect();
    assert_eq!(
        renamed_events.len(),
        1,
        "the property spelling must emit `renamed` exactly as `set_name` does — \
         the rename signals live in `SceneTree::set_node_name`, so a write that \
         bypasses it renames silently at best; got {renamed_events:?}"
    );
    assert!(
        renamed_events[0].ends_with("ByProperty"),
        "`renamed` must be emitted on the RENAMED node, not on the tree root — \
         the root-level `node_renamed` already existed and is not what a script \
         connects to; got {renamed_events:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `replace_by` must announce itself on the node being replaced.
///
/// Godot emits `replacing_by(node)` on the OUTGOING node before the splice
/// (`node.cpp:3246`). It was registered in the ClassDB and never emitted.
///
/// ⚠️ The ordering matters as much as the emission: a handler has to still see
/// `old` in place, with its children and its index, because that is the only
/// moment that information exists. Asserted below by reading `get_parent()`
/// from inside the handler.
const GD_REPLACE: &str = "
func _physics_process(delta):
    if self.phase == 0:
        self.phase = 1
        connect_all()
        return
    if self.phase == 1:
        self.phase = 2
        self.held.replace_by(get_node(\"../Spare\"), false)
        return
";

#[test]
fn replace_by_emits_replacing_by_before_the_splice() {
    let _guard = boot_lock();
    let dir = unique_project_dir("replace");
    write_project(&dir, &format!("{PROLOGUE}{GD_REPLACE}"));
    let mut boot = boot(&dir);
    let flag = flag_node(&boot);

    for _ in 0..3 {
        boot.main_loop_mut().unwrap().step(DT);
    }

    // ⚠️ Exactly one connection is made, by `connect_all`, so the count is 1.
    // An earlier draft connected a second handler and asserted 2 — which would
    // have been asserting that connecting one signal twice works, a claim this
    // test has no business resting on.
    assert_eq!(
        count(&boot, flag, "replacing_by"),
        1,
        "`replace_by` must emit `replacing_by` on the outgoing node, reaching \
         its connected handler; 0 means it is still never emitted"
    );
    assert_eq!(
        count(&boot, flag, "still_parented"),
        1,
        "`replacing_by` must be emitted BEFORE the splice, while the outgoing \
         node still has its parent — a handler that sees a detached node cannot \
         read the position it is being replaced at"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
