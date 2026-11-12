//! pat-s3rrv.226 — `FileSystemDock` is reachable from script and its signals
//! actually reach a connected callback.
//!
//! Neither `FileSystemDock` nor `EditorFileSystemImportFormatSupportQuery` was
//! registered. The recorded reason in `class_db.rs` — "needs `EditorDock` …
//! both bases are still missing" — had gone **stale**: `EditorDock` landed in
//! `editor_plugin_classes.rs`, so the parent chain was available. That note is
//! corrected as part of this bead.
//!
//! ⚠️ **A second staleness, and it mattered more.**
//! `register_editor_plugin_classes()` was reachable only through
//! `EditorState::new()`, never from `register_all_classes()` — so every test and
//! every headless consumer saw *no* `EditorDock` at all. That file's own module
//! doc asks for the missing wiring line and records that it was left undone
//! because `class_db.rs` was leased. It is wired now, which is why the
//! registration assertions below can pass at all.

use gdobject::class_db::{class_exists, is_parent_class, register_all_classes};
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Serializes the tests in this binary that touch the FileSystemDock signal queue (pat-hukbz).
///
/// That state is process-global. Nextest gives each test its own process, but
/// plain `cargo test` runs the tests as threads in ONE process, where they
/// overwrote each other's state mid-test. Bind the guard for the whole test.
/// Poison-tolerant, so one failing test does not fail the rest.
#[must_use = "bind it for the whole test: `let _guard = serial();`"]
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

const SCRIPT: &str = "\
var moved_old = \"\"
var moved_new = \"\"
var hits = 0
func _on_moved(old_path, new_path):
    self.moved_old = old_path
    self.moved_new = new_path
    self.hits += 1
func setup():
    var dock = EditorInterface.get_file_system_dock()
    dock.connect(\"files_moved\", _on_moved)
    return dock != null
func pump():
    var dock = EditorInterface.get_file_system_dock()
    dock.navigate_to_path(\"res://scenes\")
    return [self.moved_old, self.moved_new, self.hits, dock._navigated_path()]
";

// ---------------------------------------------------------------------------
// ClassDB
// ---------------------------------------------------------------------------

#[test]
fn both_classes_are_registered_under_the_right_bases() {
    register_all_classes();
    assert!(
        class_exists("FileSystemDock"),
        "FileSystemDock must exist — its parent chain has been available since \
         EditorDock landed"
    );
    assert!(
        is_parent_class("FileSystemDock", "EditorDock"),
        "Godot derives FileSystemDock from EditorDock"
    );
    assert!(
        class_exists("EditorFileSystemImportFormatSupportQuery"),
        "the import-format query class must exist"
    );
    assert!(
        is_parent_class("EditorFileSystemImportFormatSupportQuery", "RefCounted"),
        "it is a RefCounted"
    );
}

#[test]
fn the_parent_chain_reaches_control() {
    // ⚠️ `class_exists` alone is satisfied by a wrong parent. EditorDock is a
    // MarginContainer, so the chain must reach Control — that is what proves
    // the registration is wired to the real hierarchy and not floating.
    register_all_classes();
    assert!(
        is_parent_class("FileSystemDock", "MarginContainer"),
        "EditorDock is a MarginContainer, so the dock inherits one"
    );
    assert!(
        is_parent_class("FileSystemDock", "Control"),
        "and therefore a Control"
    );
}

#[test]
fn the_editor_dock_family_is_visible_to_register_all_classes() {
    // The regression guard for the wiring this bead added. Before it,
    // `register_all_classes()` left every editor_plugin_classes.rs class
    // unregistered and this was false.
    register_all_classes();
    assert!(
        class_exists("EditorDock"),
        "register_all_classes() must reach the editor plugin family, or every \
         headless consumer sees an editor with no docks"
    );
}

// ---------------------------------------------------------------------------
// Script surface and signals
// ---------------------------------------------------------------------------

/// One interpreter and one instance, so state survives between calls.
fn instance() -> (Interpreter, gdscript_interop::interpreter::ClassInstance) {
    register_all_classes();
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(SCRIPT).expect("script parses");
    let inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    (interp, inst)
}

#[test]
fn get_file_system_dock_returns_a_dock_a_script_can_use() {
    let _guard = serial();
    let (mut interp, mut inst) = instance();
    let ok = interp
        .call_instance_method(&mut inst, "setup", &[])
        .expect("setup() runs");
    assert_eq!(
        ok,
        Variant::Bool(true),
        "EditorInterface.get_file_system_dock() must return an object, not null"
    );
}

#[test]
fn a_connected_script_receives_files_moved_from_the_editor_server() {
    let _guard = serial();
    let (mut interp, mut inst) = instance();
    interp
        .call_instance_method(&mut inst, "setup", &[])
        .expect("setup() runs");

    // This is the exact call the editor server's `/api/filesystem/rename` route
    // makes after a successful `std::fs::rename`.
    gdscript_interop::emit_filesystem_dock_signal(
        "files_moved",
        vec![
            Variant::String("res://old.tscn".into()),
            Variant::String("new.tscn".into()),
        ],
    );

    let out = interp
        .call_instance_method(&mut inst, "pump", &[])
        .expect("pump() runs");
    let Variant::Array(v) = out else {
        panic!("pump() should return an array, got {out:?}");
    };
    assert_eq!(
        v.to_vec()[0],
        Variant::String("res://old.tscn".into()),
        "the callback must receive the old path"
    );
    assert_eq!(
        v.to_vec()[1],
        Variant::String("new.tscn".into()),
        "and the new one"
    );
    assert_eq!(v.to_vec()[2], Variant::Int(1), "fired exactly once");
    assert_eq!(
        v.to_vec()[3],
        Variant::String("res://scenes".into()),
        "navigate_to_path must record where the script asked the dock to go"
    );
}

#[test]
fn an_unconnected_signal_reaches_nobody_and_does_not_raise() {
    let _guard = serial();
    // The control: a queued signal with no listener must drain harmlessly
    // rather than erroring or landing on the wrong callback.
    let (mut interp, mut inst) = instance();
    interp
        .call_instance_method(&mut inst, "setup", &[])
        .expect("setup() runs");
    gdscript_interop::emit_filesystem_dock_signal(
        "folder_removed",
        vec![Variant::String("res://gone".into())],
    );
    let out = interp
        .call_instance_method(&mut inst, "pump", &[])
        .expect("pump() runs");
    let Variant::Array(v) = out else {
        panic!("expected an array, got {out:?}");
    };
    assert_eq!(
        v.to_vec()[2],
        Variant::Int(0),
        "a signal nobody connected must not invoke the files_moved callback"
    );
}
