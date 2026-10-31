//! pat-mgixk.106 acceptance: the builtin constructors a scene script writes.
//!
//! # What was wrong
//!
//! `Expr::Call(Ident)` resolves through `try_builtin`, which had arms for `int`
//! and `float` but **none** for `String`, `Array`, `Dictionary`, `bool` or
//! `Signal` — so `String(42)` and `bool(0)` fell through to
//! `call_self_func_or_native` and raised `UndefinedFunction`. `Vector2` and
//! `Vector3` accepted only 0 or 2/3 numeric arguments, so
//! `Vector2(Vector2i(1, 2))` raised *"Vector2() takes 0 or 2 arguments"*: a
//! script could narrow a vector but never widen one.
//!
//! # ⚠️ How the raising clause is observed
//!
//! A GDScript error aborts the calling frame, so there is no "it raised" flag to
//! read. Instead `ok` is set on the line BEFORE the call that must fail, and
//! `after_bad` on the line after. Asserting `ok == 1 && after_bad == 0` pins the
//! abort to exactly that statement — a passing `Array(["x"], TYPE_INT, …)` would
//! set `after_bad`, and a failure anywhere earlier would leave `ok` at 0.
//!
//! # ⚠️ Why `_process` and not `_ready`
//!
//! `patina-runner`'s boot calls `LifecycleManager::enter_tree` directly
//! (`bootstrap.rs:562`) rather than `MainLoop::enter_tree_with_scripts`, so
//! `_ready` runs outside the bracket that installs the script-facing servers.
//! `Signal(self, …)` wants a live object, so one `step()` is the honest phase.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk106_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture project dir");
    dir
}

/// Reads a script variable's CURRENT value off the node's script instance.
///
/// ⚠️ `ScriptInstance::get_property` is `interpreter.environment.get(..)` — the
/// live scope. NOT `list_properties().default_value`, whose liveness depends on
/// which `ScriptInstance` impl backs the node.
fn script_var(tree: &gdscene::SceneTree, node_id: gdscene::node::NodeId, var: &str) -> Variant {
    tree.get_script(node_id)
        .and_then(|s| s.get_property(var))
        .unwrap_or(Variant::Nil)
}

/// ⚠️ `len(..)` for the array length rather than `.size()`: `len` is a
/// documented `try_builtin` arm, so a failure here can only be about the
/// constructor under test.
const PROBE_GD: &str = r#"extends Node

var s_int = "?"
var s_vec = "?"
var b_zero = true
var b_three = false
var packed_len = -1
var copy_len = -1
var typed_is = false
var untyped_is = true
var dict_size = -1
var sig_name = "?"
var sig_null = true
var v2 = Vector2(0, 0)
var v3 = Vector3(0, 0, 0)
var ok = 0
var after_bad = 0

func _process(_delta):
    if ok == 1:
        return

    s_int = String(42)
    s_vec = String(Vector2(1, 2))

    b_zero = bool(0)
    b_three = bool(3)

    packed_len = len(Array(PackedInt32Array([1, 2])))
    copy_len = len(Array([7, 8, 9]))

    typed_is = Array([1, 2], TYPE_INT, "", null).is_typed()
    untyped_is = Array([1, 2]).is_typed()

    dict_size = Dictionary({"a": 1}).size()

    sig_name = Signal(self, "ready").get_name()
    sig_null = Signal().is_null()

    v2 = Vector2(Vector2i(3, 4))
    v3 = Vector3(Vector3i(1, 2, 3))

    ok = 1
    # ⚠️ Must RAISE: a String element in an int-typed Array. Everything after
    # this line is unreachable, which is how the test observes the error.
    var bad = Array(["x"], TYPE_INT, "", null)
    after_bad = 1
"#;

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"BuiltinCtors\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .expect("write project.godot");
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://probe.gd\" id=\"1_probe\"]\n\n\
         [node name=\"Main\" type=\"Node\"]\n\
         script = ExtResource(\"1_probe\")\n",
    )
    .expect("write main.tscn");
    std::fs::write(dir.join("probe.gd"), PROBE_GD).expect("write probe.gd");
}

/// **The acceptance test.**
#[test]
fn builtin_constructors_test() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("ctors");
    write_project(&dir);

    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.clone(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot flow should complete");
    boot.main_loop_mut()
        .expect("the Running phase creates a MainLoop")
        .step(1.0 / 60.0);

    let tree = boot.main_loop().expect("MainLoop").tree();
    let main = tree
        .get_node_by_path("/root/Main")
        .expect("main.tscn instanced its root");
    let var = |k: &str| script_var(tree, main, k);

    // -- 0. everything up to the deliberate failure ran --------------------
    //
    // 🔑 First, because a missing constructor raises `UndefinedFunction` and
    // ABORTS the frame: every variable below would keep its declared default,
    // and several of those defaults are the values a passing test expects.
    assert_eq!(
        var("ok"),
        Variant::Int(1),
        "the script must reach the line before the deliberate failure; a 0 \
         means one of the constructors above raised"
    );

    // -- 1. String(any) ----------------------------------------------------
    assert_eq!(
        var("s_int"),
        Variant::String("42".into()),
        "String(42) stringifies through the same path `print`/`str` use"
    );
    // ⚠️ The Vector2 rendering is NOT asserted verbatim: the exact punctuation
    // is `display_variant`'s business and pinning it here would couple this
    // test to a formatting choice it is not about. Both components must appear.
    match var("s_vec") {
        Variant::String(s) => assert!(
            s.contains('1') && s.contains('2'),
            "String(Vector2(1, 2)) must render both components; got {s:?}"
        ),
        other => panic!("`s_vec` should be a String, got {other:?}"),
    }

    // -- 2. bool(int) ------------------------------------------------------
    assert_eq!(
        var("b_zero"),
        Variant::Bool(false),
        "bool(0) is false — the missing third of the int/float/bool trio"
    );
    assert_eq!(
        var("b_three"),
        Variant::Bool(true),
        "bool(3) is true; asserted alongside bool(0) so a constructor that \
         always answered false could not pass"
    );

    // -- 3. Array from a packed array, and the plain copy ------------------
    assert_eq!(
        var("packed_len"),
        Variant::Int(2),
        "Array(PackedInt32Array([1, 2])) unpacks elementwise — the only way a \
         script reaches a packed array's contents as Variants"
    );
    assert_eq!(
        var("copy_len"),
        Variant::Int(3),
        "Array([7, 8, 9]) copies the elements"
    );

    // -- 4. 🔑 the typed constructor, both ways ----------------------------
    assert_eq!(
        var("typed_is"),
        Variant::Bool(true),
        "Array([1, 2], TYPE_INT, \"\", null) must be typed"
    );
    // The negative half: without it, an `is_typed` that always answered true,
    // or a constructor that typed every array, would satisfy the clause above.
    assert_eq!(
        var("untyped_is"),
        Variant::Bool(false),
        "a plain Array([1, 2]) is UNTYPED — is_typed must discriminate"
    );

    // -- 5. Dictionary(from) -----------------------------------------------
    assert_eq!(
        var("dict_size"),
        Variant::Int(1),
        // ⚠️ Braces DOUBLED: an assertion message is a format string, so a
        // literal `{` must be `{{` or it is read as a placeholder.
        "Dictionary({{\"a\": 1}}) copies the entries"
    );

    // -- 6. Signal(object, name) -------------------------------------------
    assert_eq!(
        var("sig_name"),
        Variant::String("ready".into()),
        "Signal(self, \"ready\").get_name() returns the bound name"
    );
    // ⚠️ The null signal matters: a `Signal` carries the OBJECT, and one bound
    // to nothing must report itself as null rather than looking like a valid
    // signal that happens to be empty.
    assert_eq!(
        var("sig_null"),
        Variant::Bool(true),
        "Signal() with no arguments is the null signal"
    );

    // -- 7. the vector widening forms --------------------------------------
    assert_eq!(
        var("v2"),
        Variant::Vector2(gdcore::math::Vector2::new(3.0, 4.0)),
        "Vector2(Vector2i(3, 4)) widens; this raised \"takes 0 or 2 \
         arguments\" before"
    );
    assert_eq!(
        var("v3"),
        Variant::Vector3(gdcore::math::Vector3::new(1.0, 2.0, 3.0)),
        "Vector3(Vector3i(1, 2, 3)) widens"
    );

    // -- 8. 🔑 the typed constructor REFUSES a mismatched element ----------
    //
    // `assign` validates every element against the array's type, so a String
    // in an int-typed array is a script error. Observed as an abort: the
    // statement after it never ran.
    assert_eq!(
        var("after_bad"),
        Variant::Int(0),
        "Array([\"x\"], TYPE_INT, \"\", null) must RAISE. A 1 here means it \
         quietly produced a typed array holding a String, which is the defect \
         the typing is supposed to prevent"
    );
}
