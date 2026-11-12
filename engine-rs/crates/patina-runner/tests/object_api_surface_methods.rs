//! pat-mgixk.99 — Godot's `Object` API surface, driven from a booted project's
//! GDScript: user signals, `set_block_signals`, queued deletion, `set_script`,
//! `notification()` and message translation.
//!
//! # What was broken
//!
//! None of these dispatched. Eleven were absent from ClassDB entirely, and
//! `notification` was registered (`class_db.rs:1073`) with no arm — so it took
//! the one path that is NOT silent: being non-void, the interpreter RAISED
//! rather than answering Nil, and `_notification` was never invoked. The rest
//! answered Nil through the `responds` branch, so a script could call
//! `set_block_signals(true)` and see no error and no effect.
//!
//! # How state is read back
//!
//! Everything is recorded onto a `Flag` node's META and read from Rust, which
//! is the pattern the green `gdscript_camera3d_projection_methods` uses.
//! ⚠️ Deliberately NOT script vars written from the signal handler: a handler's
//! `self.*` writes do not survive its return, so `_on_boom` would appear never
//! to have run. `done` is written LAST so a failure distinguishes "every call
//! answered" from "the frame died at the first one".
//!
//! # ⚠️ Why `notification()` is called on ANOTHER node
//!
//! `SceneTree::notify_script` opens with `self.scripts.remove(&node_id)` — the
//! borrow-safe accessor dance that lets `_notification` call back into the tree.
//! A script callback is already running under that same dance, so its own script
//! is ALREADY out of the map: a bare `notification(..)` on `self` from inside
//! `_process` finds nothing to deliver to and is a silent no-op.
//!
//! So clause 5 notifies `Plain`, which has just received a script via
//! `set_script` and whose script IS in the map. That exercises the method's real
//! contract — reaching a script's `_notification` with the right `what` —
//! without depending on re-entrancy the engine does not support.
//!
//! The self-call limitation is REAL and is reported on the bead. It is
//! deliberately NOT asserted here: pinning it with a test would enshrine a wart
//! as a contract.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// Booting touches engine-wide state (ClassDB, the translation server), so the
/// cases are serialized.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

const DT: f64 = 1.0 / 60.0;

/// The key the seeded catalog translates, and what it translates to.
const TR_KEY: &str = "OBJ_API_KEY";
const TR_VALUE: &str = "translated";

/// A catalog so that clause 6 is a real differential.
///
/// 🔑 Without this the clause would be VACUOUS: `tr()` returns its key
/// unchanged when nothing is loaded, so `set_message_translation(false)` would
/// "pass" against a `tr` that never translated anything in the first place.
///
/// ⚠️ Seeded directly into the engine-wide `TranslationServer` rather than
/// through `project.godot`, because **`EngineBootstrap` does not load
/// translations at all** — `grep -n translation crates/patina-runner/src/bootstrap.rs`
/// is empty; only `project_boot.rs:177` reads
/// `internationalization/locale/translations`. That is the same structural gap
/// pat-mgixk.88 found for the audio settings, and it is reported on the bead
/// rather than worked around silently. Seeding the catalog is fixture setup,
/// like writing the `.tscn`; the API under test is still entered from the
/// script.
fn seed_translation() {
    let po = format!(
        "msgid \"\"\nmsgstr \"\"\n\"Language: en\\n\"\n\nmsgid \"{TR_KEY}\"\nmsgstr \"{TR_VALUE}\"\n"
    );
    gdcore::translation::with_translation_server_mut(|server| {
        server.add_translation(gdcore::translation::parse_po("en", &po));
        server.set_locale("en");
    });
}

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk99_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create project dir");
    dir
}

/// `Main (Node3D, scripted)` with three plain children: `Flag` records results,
/// `Doomed` is queued and then rescued, `Plain` receives a script at runtime.
///
/// 🔑 `Plain` carries NO `script =` of its own — that is the whole point of the
/// `set_script` clause, and a scene-authored script would make it pass without
/// `set_script` doing anything.
fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .expect("write project.godot");
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1_m\"]\n\n\
         [node name=\"Main\" type=\"Node3D\"]\n\
         script = ExtResource(\"1_m\")\n\n\
         [node name=\"Flag\" type=\"Node3D\" parent=\".\"]\n\n\
         [node name=\"Doomed\" type=\"Node3D\" parent=\".\"]\n\n\
         [node name=\"Plain\" type=\"Node3D\" parent=\".\"]\n",
    )
    .expect("write main.tscn");
    std::fs::write(dir.join("s.gd"), SCRIPT_S).expect("write s.gd");
    std::fs::write(dir.join("main.gd"), MAIN_GD).expect("write main.gd");
}

/// The script `set_script` attaches. `from_s` is the method whose presence
/// proves the attach landed.
const SCRIPT_S: &str = "\
extends Node3D

func from_s():
    return 99

func _notification(what):
    if what == 1234:
        var f = get_node_or_null(\"../Flag\")
        if f:
            f.set_meta(\"notified\", what)
";

/// ⚠️ The self-calls here are the BARE form on purpose — `add_user_signal(..)`,
/// `set_block_signals(..)`, `notification(..)`, `set_message_translation(..)`.
/// That is a different interpreter match from `$Node.method()`, so testing only
/// the qualified form would leave half the dispatch unexercised.
const MAIN_GD: &str = "\
extends Node3D
var phase = 0

func _on_boom(x):
    var f = get_node_or_null(\"Flag\")
    if f:
        f.set_meta(\"got\", x)
        f.set_meta(\"calls\", f.get_meta(\"calls\") + 1)

func _process(delta):
    if self.phase != 0:
        return
    self.phase = 1
    var f = get_node(\"Flag\")
    f.set_meta(\"done\", 0)
    f.set_meta(\"calls\", 0)

    add_user_signal(\"boom\", [{\"name\": \"x\", \"type\": TYPE_INT}])
    f.set_meta(\"has_user\", has_user_signal(\"boom\"))
    f.set_meta(\"has_unknown\", has_user_signal(\"never_added\"))
    connect(\"boom\", Callable(self, \"_on_boom\"))
    emit_signal(\"boom\", 3)
    f.set_meta(\"calls_before_block\", f.get_meta(\"calls\"))

    set_block_signals(true)
    f.set_meta(\"blocking\", is_blocking_signals())
    emit_signal(\"boom\", 7)
    f.set_meta(\"calls_after_block\", f.get_meta(\"calls\"))
    set_block_signals(false)
    f.set_meta(\"blocking_after\", is_blocking_signals())

    var d = get_node(\"Doomed\")
    d.queue_free()
    f.set_meta(\"queued\", d.is_queued_for_deletion())
    d.cancel_free()
    f.set_meta(\"after_cancel\", d.is_queued_for_deletion())

    var p = get_node(\"Plain\")
    f.set_meta(\"had_from_s\", p.has_method(\"from_s\"))
    f.set_meta(\"set_ok\", p.set_script(load(\"res://s.gd\")))
    f.set_meta(\"has_from_s\", p.has_method(\"from_s\"))

    p.notification(1234)

    f.set_meta(\"tr_on\", tr(\"OBJ_API_KEY\"))
    set_message_translation(false)
    f.set_meta(\"can_tr\", can_translate_messages())
    f.set_meta(\"tr_off\", tr(\"OBJ_API_KEY\"))

    f.set_meta(\"done\", 1)
";

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

fn child(boot: &EngineBootstrap, name: &str) -> gdscene::node::NodeId {
    let ml = boot
        .main_loop()
        .expect("the Running phase creates a MainLoop");
    let root = ml.tree().root_id();
    let main = ml
        .tree()
        .find_child(root, "Main", true)
        .expect("the main scene instantiated");
    ml.tree()
        .find_child(main, name, true)
        .unwrap_or_else(|| panic!("the scene has a `{name}` node"))
}

fn meta(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> Variant {
    boot.main_loop()
        .expect("MainLoop")
        .tree()
        .get_node(flag)
        .expect("the Flag node is still in the tree")
        .get_meta(key)
}

/// **The acceptance test.**
#[test]
fn object_api_surface_methods() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    seed_translation();

    let dir = unique_project_dir("main");
    write_project(&dir);
    // 🔑 REQUIRED, and the first run of this test failed for exactly its
    // absence: `set_script(load("res://s.gd"))` returned false because
    // `res://` could not resolve.
    //
    // ⚠️ `gdcore::set_path_roots` is called ONLY from
    // `patina-runner/src/project_boot.rs` (`:74`, `:142`, `:516`) and NEVER from
    // `bootstrap.rs`, so a project booted through `EngineBootstrap` has
    // `project_root() == None` and every `res://` read fails. A real game boots
    // through `main.rs` -> `project_boot`, where this is set; this line mirrors
    // that, exactly as the translation seeding above mirrors the catalog load
    // the same boot path skips. Reported on the bead as a third instance of the
    // same gap, not fixed here — adding it to `bootstrap.rs` would change
    // `res://` behaviour for every test that boots this way.
    gdcore::set_path_roots(Some(dir.to_path_buf()), None);
    let mut boot = boot(&dir);

    let flag = child(&boot, "Flag");
    let doomed = child(&boot, "Doomed");

    // Several frames: `_process` runs on the first, and the extra ones let
    // `process_deletions` run so the `cancel_free` clause is observable.
    for _ in 0..4 {
        boot.main_loop_mut().expect("MainLoop").step(DT);
    }

    // -- 0. the frame survived every call ------------------------------------
    //
    // Checked first. Every assertion below reads a meta key this script writes,
    // and a raise part-way through leaves the later keys `Nil` — which would
    // otherwise read as "the method answered wrongly" instead of "the frame
    // died earlier".
    assert_eq!(
        meta(&boot, flag, "done"),
        Variant::Int(1),
        "`_process` must reach its last line. `Nil` or 0 means one of the \
         Object methods raised and aborted the frame — which is exactly what an \
         undispatched, non-void registration does (`notification` was the one \
         already in ClassDB)"
    );

    // -- 1. user signals -----------------------------------------------------
    assert_eq!(
        meta(&boot, flag, "has_user"),
        Variant::Bool(true),
        "has_user_signal must report the signal add_user_signal just created"
    );
    assert_eq!(
        meta(&boot, flag, "has_unknown"),
        Variant::Bool(false),
        "has_user_signal must be FALSE for a name never added — otherwise the \
         clause above would pass against a stub that answers true for anything"
    );
    assert_eq!(
        meta(&boot, flag, "got"),
        Variant::Int(3),
        "the handler must receive the emitted 3, so the runtime-added signal is \
         connectable and emittable like a declared one"
    );
    assert_eq!(
        meta(&boot, flag, "calls_before_block"),
        Variant::Int(1),
        "the handler must have run exactly once before signals were blocked"
    );

    // -- 2. 🔑 set_block_signals actually suppresses delivery -----------------
    assert_eq!(
        meta(&boot, flag, "blocking"),
        Variant::Bool(true),
        "is_blocking_signals must report the flag just set"
    );
    assert_eq!(
        meta(&boot, flag, "calls_after_block"),
        Variant::Int(1),
        "the emit while blocked must NOT reach the handler, so the call count \
         must still be 1. Seeing 2 means `emit_signal` ignored the block — \
         Godot's `Object::emit_signalp` returns early (object.cpp:1275)"
    );
    assert_eq!(
        meta(&boot, flag, "blocking_after"),
        Variant::Bool(false),
        "the flag must clear again; a latched block would silently kill every \
         later signal on this object"
    );

    // -- 3. queued deletion and cancel_free ----------------------------------
    assert_eq!(
        meta(&boot, flag, "queued"),
        Variant::Bool(true),
        "is_queued_for_deletion must be true right after queue_free"
    );
    assert_eq!(
        meta(&boot, flag, "after_cancel"),
        Variant::Bool(false),
        "cancel_free must clear the pending deletion"
    );
    // 🔑 The real observable: the node is still in the tree after the frames
    // that run `process_deletions`. The flags above could both be right while
    // the node still died.
    assert!(
        boot.main_loop()
            .expect("MainLoop")
            .tree()
            .get_node(doomed)
            .is_some(),
        "the cancelled node must survive the deletion pass; if it is gone, \
         cancel_free reported success and the node died anyway"
    );

    // -- 4. set_script ------------------------------------------------------
    assert_eq!(
        meta(&boot, flag, "had_from_s"),
        Variant::Bool(false),
        "the Plain node must NOT have `from_s` before set_script — the scene \
         authors no script on it, and this is the control that makes the next \
         assertion meaningful"
    );
    // 🔑 The return value FIRST, so a failure distinguishes "the script could
    // not be loaded" from "it attached but `has_method` cannot see it". The
    // first run of this test had no such split and reported only the symptom.
    assert_eq!(
        meta(&boot, flag, "set_ok"),
        Variant::Bool(true),
        "set_script must report success. false means the `res://s.gd` read or \
         the parse failed, not that `has_method` is blind — check that \
         `gdcore::set_path_roots` was called, since `EngineBootstrap` does not"
    );
    assert_eq!(
        meta(&boot, flag, "has_from_s"),
        Variant::Bool(true),
        "after set_script(load(\"res://s.gd\")) the node must report the \
         script's method"
    );

    // -- 5. notification() reaches _notification -----------------------------
    assert_eq!(
        meta(&boot, flag, "notified"),
        Variant::Int(1234),
        "notification(1234) must invoke _notification with what == 1234. `Nil` \
         means it never arrived — this is the one method that was already in \
         ClassDB, so before the change it RAISED rather than returning Nil"
    );

    // -- 6. message translation ---------------------------------------------
    assert_eq!(
        meta(&boot, flag, "tr_on"),
        Variant::String(TR_VALUE.to_string()),
        "with translation enabled, tr() must return the seeded catalog's value. \
         Getting the key back means the catalog never loaded and the clause \
         below would be vacuous"
    );
    assert_eq!(
        meta(&boot, flag, "can_tr"),
        Variant::Bool(false),
        "can_translate_messages must report the flag set_message_translation just cleared"
    );
    assert_eq!(
        meta(&boot, flag, "tr_off"),
        Variant::String(TR_KEY.to_string()),
        "after set_message_translation(false), tr() must return the KEY \
         unchanged rather than the translation"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
