//! **Acceptance test for pat-mgixk.98, clause 2.** Godot's `ConnectFlags`
//! semantics, end to end from a booted project's script.
//!
//! # What was broken
//!
//! * `Signal::connect` pushed unconditionally, under a doc comment claiming
//!   "Duplicates (same target + method) are allowed, matching Godot's default
//!   behavior". Backwards: `Object::connect`
//!   (`core/object/object.cpp:1646-1666`) **errors** on an already-connected
//!   `(target, method)` pair. A re-entered `_ready` that connected twice fired
//!   its handler twice on every emit.
//! * `CONNECT_REFERENCE_COUNTED` was ignored outright, so the documented escape
//!   hatch for intentional duplicates did nothing.
//! * Connecting to a signal the object does not have **lazily created** it
//!   (`SignalStore::connect`), so a mistyped signal name connected
//!   "successfully" and then simply never fired — the quietest failure there
//!   is. Godot answers `ERR_INVALID_PARAMETER` (`object.cpp:1638`).
//! * `CONNECT_APPEND_SOURCE_OBJECT` (16) did not exist anywhere: a script
//!   naming it got `Nil`, so the call passed **no flags at all**.
//! * `get_signal_connection_list` reported `signal` and `callable` as plain
//!   Strings — a script could not `.connect()` or `.call()` what it got back —
//!   and `flags` as `i64::from(one_shot)`, i.e. `1`, under a comment asserting
//!   "ONE_SHOT is 1". Godot's `ONE_SHOT` is **4**; `1` is `CONNECT_DEFERRED`.
//!   So a one-shot connection described itself as deferred, and a deferred one
//!   as nothing.
//!
//! # Wiring
//!
//! Boots a real project through `patina_runner` and asserts only what the
//! script can see.
//!
//! ⚠️ Every emit is driven from the test, never from inside a script method: a
//! currently-running script is checked out of `SceneTree.scripts`, so a signal
//! emitted from within `_ready` could not be delivered back into that same
//! script. `signal_value_connect_emit.rs:20-24` documents the same constraint.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

static BOOT_LOCK: Mutex<()> = Mutex::new(());

/// Godot `Error::ERR_INVALID_PARAMETER` (`core/error/error_list.h`).
const ERR_INVALID_PARAMETER: i64 = 31;
/// Godot `Object::CONNECT_ONE_SHOT` (`core/object/object.h:576`).
///
/// ⚠️ **4**, not 1. The value this test pins is exactly the one the old code
/// got wrong: it reported `1`, which is `CONNECT_DEFERRED`.
const CONNECT_ONE_SHOT: i64 = 4;
/// `TYPE_CALLABLE` from `@GlobalScope.Variant.Type`.
const TYPE_CALLABLE: i64 = 25;
/// The argument the `src` signal is emitted with, ahead of the appended source.
/// ⚠️ Not 0 or -1: the vars it lands in start at -1.
const SRC_ARG: i64 = 9;

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk98_flags_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const MAIN_TSCN: &str = r#"[gd_scene load_steps=3 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]
[ext_resource type="Script" path="res://emitter.gd" id="2_e"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="E" type="Node2D" parent="."]
script = ExtResource("2_e")
"#;

/// The four signals under test. ⚠️ `no_such_signal` is deliberately NOT here —
/// that is the whole point of the validation clause.
const EMITTER_GD: &str = "\
extends Node2D
signal dup
signal rc
signal src
signal once
";

/// ⚠️ Handler writes are BARE, not `self.`-prefixed:
/// `object_api_surface_methods.rs:18-20` records that a handler's `self.*`
/// writes do not survive its return, while the green signal-handler reference
/// `signal_value_connect_emit.rs` uses bare assignment and passes. `_ready` and
/// `drop_rc()` are not handlers.
const MAIN_GD: &str = "\
extends Node2D

var dup_first = -1
var dup_second = -1
var dup_hits = 0
var rc_first = -1
var rc_second = -1
var rc_hits = 0
var bad_code = -1
var src_a = -1
var src_got = null
var list_size = -1
var list_flags = -1
var list_callable_type = -1
var ready_done = false

func _ready():
    dup_first = $E.connect(\"dup\", _on_dup)
    dup_second = $E.connect(\"dup\", _on_dup)
    rc_first = $E.connect(\"rc\", _on_rc, CONNECT_REFERENCE_COUNTED)
    rc_second = $E.connect(\"rc\", _on_rc, CONNECT_REFERENCE_COUNTED)
    bad_code = $E.connect(\"no_such_signal\", _on_dup)
    $E.connect(\"src\", _on_src, CONNECT_APPEND_SOURCE_OBJECT)
    $E.connect(\"once\", _on_once, CONNECT_ONE_SHOT)
    var lst = $E.get_signal_connection_list(\"once\")
    list_size = lst.size()
    if list_size > 0:
        list_flags = lst[0][\"flags\"]
        list_callable_type = typeof(lst[0][\"callable\"])
    ready_done = true

func _on_dup():
    dup_hits += 1

func _on_rc():
    rc_hits += 1

func _on_once():
    dup_hits += 100

func _on_src(a, s):
    src_a = a
    src_got = s

func drop_rc():
    $E.disconnect(\"rc\", _on_rc)
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"ConnectFlags\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("main.gd"), MAIN_GD).unwrap();
    std::fs::write(dir.join("emitter.gd"), EMITTER_GD).unwrap();
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

/// ⚠️ `get_property`, NOT `list_properties().default_value` — the latter is
/// parse-time metadata and would report each variable's declared initializer
/// forever, so every assertion below would read its own sentinel regardless of
/// what the engine did.
fn script_var(tree: &gdscene::SceneTree, node: gdscene::node::NodeId, var: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(var)
        .unwrap_or(Variant::Nil)
}

fn as_int(v: Variant, what: &str) -> i64 {
    match v {
        Variant::Int(i) => i,
        other => panic!("`{what}` should be an Int, got {other:?}"),
    }
}

/// **The acceptance test.**
#[test]
fn signal_connect_flag_semantics() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("main");
    write_project(&dir);

    let mut boot = boot(&dir);
    let (main_id, e_id) = {
        let tree = boot.main_loop().expect("main loop built").tree();
        (
            tree.get_node_by_path("/root/Main").expect("Main"),
            tree.get_node_by_path("/root/Main/E").expect("E"),
        )
    };

    // -- 0. `_ready` ran to completion --------------------------------------
    //
    // Checked first: a raise aborts the rest of `_ready`, so every var below
    // would read its sentinel and look like six independent failures instead of
    // one. At HEAD the likely culprit is `CONNECT_APPEND_SOURCE_OBJECT`, which
    // did not exist as a global constant.
    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            script_var(tree, main_id, "ready_done"),
            Variant::Bool(true),
            "_ready must run to completion; `false` means it raised partway"
        );
    }

    // -- 1. a duplicate connect is REFUSED ----------------------------------
    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "dup_first"), "dup_first"),
            0,
            "the first connect must succeed with OK"
        );
        assert_eq!(
            as_int(script_var(tree, main_id, "dup_second"), "dup_second"),
            ERR_INVALID_PARAMETER,
            "connecting the same Callable twice must report an error. 0 here \
             means the duplicate was accepted — the behaviour the old \
             `Signal::connect` had, which fired the handler twice per emit"
        );
    }

    // ...and the handler fires ONCE per emit, which is the observable half:
    // an error code with two connections still stored would be a lie.
    boot.main_loop_mut()
        .expect("main loop")
        .tree_mut()
        .emit_signal(e_id, "dup", &[]);
    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "dup_hits"), "dup_hits"),
            1,
            "one emit must reach the handler once; 2 means the refused \
             duplicate was stored anyway. 🔑 101 would mean something else: \
             `_on_once` adds 100, so that value says the ONE_SHOT connect \
             landed on `dup` instead of `once`"
        );
    }

    // -- 2. 🔑 CONNECT_REFERENCE_COUNTED counts instead of refusing ----------
    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "rc_first"), "rc_first"),
            0,
            "the first reference-counted connect must succeed"
        );
        assert_eq!(
            as_int(script_var(tree, main_id, "rc_second"), "rc_second"),
            0,
            "with CONNECT_REFERENCE_COUNTED a duplicate is ACCEPTED, not \
             refused — {} here means the flag was ignored and the duplicate \
             rule applied anyway",
            ERR_INVALID_PARAMETER
        );
    }

    boot.main_loop_mut()
        .expect("main loop")
        .tree_mut()
        .emit_signal(e_id, "rc", &[]);
    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "rc_hits"), "rc_hits"),
            1,
            "counted twice is still ONE connection, so one emit dispatches \
             once. 2 means the refcount added a second connection instead of \
             incrementing"
        );
    }

    // One disconnect only drops the count to 1 — the connection survives.
    boot.main_loop_mut()
        .expect("main loop")
        .tree_mut()
        .call_script_method(main_id, "drop_rc", &[]);
    boot.main_loop_mut()
        .expect("main loop")
        .tree_mut()
        .emit_signal(e_id, "rc", &[]);
    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "rc_hits"), "rc_hits"),
            2,
            "a connection held twice must survive the FIRST disconnect; still \
             1 means the disconnect removed it outright and the refcount is not \
             consulted"
        );
    }

    // The second disconnect takes it to zero and removes it.
    boot.main_loop_mut()
        .expect("main loop")
        .tree_mut()
        .call_script_method(main_id, "drop_rc", &[]);
    boot.main_loop_mut()
        .expect("main loop")
        .tree_mut()
        .emit_signal(e_id, "rc", &[]);
    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "rc_hits"), "rc_hits"),
            2,
            "after the second disconnect the connection is gone, so the count \
             must not advance; 3 means it is still connected"
        );
    }

    // -- 3. connecting to a signal that does not exist fails ----------------
    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "bad_code"), "bad_code"),
            ERR_INVALID_PARAMETER,
            "connect(\"no_such_signal\", cb) must answer ERR_INVALID_PARAMETER. \
             0 means the signal was lazily CREATED, which is how a mistyped \
             signal name used to connect successfully and then never fire"
        );
    }

    // -- 4. 🔑 CONNECT_APPEND_SOURCE_OBJECT delivers the emitter LAST --------
    //
    // The handler takes two parameters and the signal carries one, so without
    // the appended source this is an arity mismatch: the call raises and
    // `src_a` keeps its sentinel.
    boot.main_loop_mut()
        .expect("main loop")
        .tree_mut()
        .emit_signal(e_id, "src", &[Variant::Int(SRC_ARG)]);
    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "src_a"), "src_a"),
            SRC_ARG,
            "the handler must receive the signal's own argument first; -1 means \
             it was never invoked, which with a 2-parameter handler and a \
             1-argument signal is what happens when no source is appended"
        );
        // 🔑 Identity, not just presence: the appended value must be the
        // EMITTER, and it must come after the signal's own argument.
        match script_var(tree, main_id, "src_got") {
            Variant::ObjectId(got) => assert_eq!(
                got.raw(),
                e_id.object_id().raw(),
                "the appended object must be the emitter `E`, not the target or \
                 the scene root"
            ),
            other => panic!(
                "`src_got` must be the emitter's ObjectId, got {other:?}. Nil \
                 means nothing was appended"
            ),
        }
    }

    // -- 5. get_signal_connection_list reports Godot's dict ------------------
    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "list_size"), "list_size"),
            1,
            "the one-shot connection must appear in the connection list"
        );
        assert_eq!(
            as_int(script_var(tree, main_id, "list_flags"), "list_flags"),
            CONNECT_ONE_SHOT,
            "the reported flags must be CONNECT_ONE_SHOT ({}). 1 is the old \
             `i64::from(one_shot)` bug, and 1 is CONNECT_DEFERRED — so a \
             one-shot connection described itself as deferred",
            CONNECT_ONE_SHOT
        );
        assert_eq!(
            as_int(
                script_var(tree, main_id, "list_callable_type"),
                "list_callable_type"
            ),
            TYPE_CALLABLE,
            "`callable` must be a Callable, not a String — a script has to be \
             able to `.call()` what the list hands back"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
