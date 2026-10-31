//! pat-mgixk.91 — the `Node3D` helper methods a script calls actually dispatch.
//!
//! All 37 are registered in the ClassDB's `Node3D` block
//! (`class_db.rs:1682-1762`), so `has_method()` has always answered true for
//! them. The bead was filed because none had a dispatch arm, which made
//! `$Pivot.hide()` answer `Nil` and leave `visible == true`.
//!
//! That is no longer the state of the engine: `node3d::call_node3d_method`
//! (`node3d.rs:2139`) is a real table, gated on `is_node3d`, and it is reached
//! from `scripting.rs:3273`. This test is the acceptance the bead named, and it
//! did not exist — so the behaviour was working but unlocked.
//!
//! # What is asserted, and how
//!
//! 🔑 Every value is recorded by the SCRIPT, into metadata on a `Flag` node, and
//! read back from Rust. The point of the bead is that **a script** can reach
//! these methods; asserting the same transforms from Rust would pass against a
//! completely undispatched engine, because the Rust helpers were never the
//! missing half.
//!
//! ⚠️ Node metadata rather than script variables: a value written to `self.*`
//! inside a handler does not survive the handler, and keeping all the recording
//! in one mechanism avoids relying on which contexts do persist.
//!
//! ⚠️ `is_visible_in_tree` is sampled on the CHILD while `hide()` is called on
//! the PARENT. That is the whole behaviour — `hide()` sets one node's `visible`
//! and `is_visible_in_tree` walks ancestors. Sampling the same node would pass
//! against an implementation that never walked anything.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `boot_project` mutates the process-global `res://` roots, so serialize under
/// the single-process runner. `nextest` isolates per process.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

const DT: f64 = 1.0 / 60.0;

/// Takes the boot lock, tolerating a previous test's panic, so one failure does
/// not turn every later test into an opaque `PoisonError`.
fn boot_lock() -> std::sync::MutexGuard<'static, ()> {
    BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk91_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `Main > Pivot > Mesh`, the shape the bead's acceptance names, plus a `Flag`
/// the script records onto.
fn write_project(dir: &Path, gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1_m\"]\n\n\
         [node name=\"Main\" type=\"Node3D\"]\n\
         script = ExtResource(\"1_m\")\n\n\
         [node name=\"Pivot\" type=\"Node3D\" parent=\".\"]\n\n\
         [node name=\"Mesh\" type=\"MeshInstance3D\" parent=\"Pivot\"]\n\n\
         [node name=\"Flag\" type=\"Node3D\" parent=\".\"]\n",
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

/// ⚠️ `done` is written LAST. Every call above it is one the bead reported as
/// undispatched, and an undispatched name that returns a VALUE raises rather
/// than answering `Nil` (`gdscript-interop/src/native_method_gaps.rs`), which
/// aborts `_process` and leaves this flag unset. So the flag distinguishes "the
/// methods answered" from "the frame died partway".
const GD: &str = "\
extends Node3D
var phase = 0

func _process(delta):
    if self.phase != 0:
        return
    self.phase = 1
    var f = get_node(\"Flag\")
    var pivot = get_node(\"Pivot\")
    var mesh = get_node(\"Pivot/Mesh\")
    f.set_meta(\"done\", 0)
    f.set_meta(\"vis_before\", mesh.is_visible_in_tree())
    pivot.hide()
    f.set_meta(\"vis_hidden\", mesh.is_visible_in_tree())
    pivot.show()
    f.set_meta(\"vis_shown\", mesh.is_visible_in_tree())
    pivot.global_translate(Vector3(1, 0, 0))
    f.set_meta(\"gx\", pivot.get_global_position().x)
    f.set_meta(\"parent_name\", pivot.get_parent_node_3d().get_name())
    pivot.orthonormalize()
    f.set_meta(\"done\", 1)
";

fn meta(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> Variant {
    boot.main_loop()
        .unwrap()
        .tree()
        .get_node(flag)
        .expect("the Flag node is still in the tree")
        .get_meta(key)
}

#[test]
fn gdscript_node3d_helper_methods_dispatch() {
    let _guard = boot_lock();
    let dir = unique_project_dir("node3d");
    write_project(&dir, GD);
    let mut boot = boot(&dir);

    let flag = boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node_by_path("/root/Main/Flag")
        .expect("the fixture declares a Flag node");

    for _ in 0..3 {
        boot.main_loop_mut().unwrap().step(DT);
    }

    // -- the frame survived every call --------------------------------------
    assert_eq!(
        meta(&boot, flag, "done"),
        Variant::Int(1),
        "`_process` must reach its last line; 0 or Nil means one of the Node3D \
         helpers above raised and aborted the frame, which is what an \
         undispatched value-returning registration does"
    );

    // -- hide()/show() on the PARENT move the CHILD's is_visible_in_tree ----
    //
    // 🔑 Three samples, not one. `true -> false -> true` rules out a reader
    // stuck on a constant, which either single assertion alone would accept.
    assert_eq!(
        meta(&boot, flag, "vis_before"),
        Variant::Bool(true),
        "a visible subtree must report `is_visible_in_tree() == true` before \
         anything hides it"
    );
    assert_eq!(
        meta(&boot, flag, "vis_hidden"),
        Variant::Bool(false),
        "`$Pivot.hide()` must make the CHILD's `is_visible_in_tree()` false — \
         the ancestor walk is the behaviour, and `Nil` here is the pre-fix \
         answer"
    );
    assert_eq!(
        meta(&boot, flag, "vis_shown"),
        Variant::Bool(true),
        "`show()` must restore it; a one-way `hide` would pass the assertion \
         above on its own"
    );

    // -- global_translate moves the node in global space --------------------
    // ⚠️ Accepts `Int` as well as `Float`. A whole-numbered component can come
    // back either way depending on how the component read is boxed, and pinning
    // the Variant KIND here would fail on a correct value of 1 — the claim under
    // test is the translation, not the numeric representation.
    let gx = match meta(&boot, flag, "gx") {
        Variant::Float(x) => x,
        Variant::Int(n) => n as f64,
        other => panic!(
            "`get_global_position().x` must answer a number; `Nil` means \
             `global_translate` or the component read was never dispatched, got \
             {other:?}"
        ),
    };
    assert!(
        (gx - 1.0).abs() < 1e-5,
        "`global_translate(Vector3(1,0,0))` must put the global x at 1.0; got {gx}"
    );

    // -- get_parent_node_3d resolves the nearest Node3D ancestor ------------
    assert_eq!(
        meta(&boot, flag, "parent_name"),
        Variant::String("Main".to_string()),
        "`get_parent_node_3d()` must answer the Pivot's Node3D parent, and a \
         real node object — a `Nil` would have failed on `.get_name()` and \
         aborted the frame instead"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
