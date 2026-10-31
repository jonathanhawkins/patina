//! pat-mgixk.54 acceptance — a booted project's script reaches the full typed
//! FileAccess surface.
//!
//! Godot's `FileAccess` exposes 68 methods (`core/io/file_access.cpp`,
//! `doc/classes/FileAccess.xml`), and save-game code is written around four of
//! them in particular: `store_line`/`get_line` and `store_var`/`get_var`.
//!
//! Patina implemented the typed primitives in `gdcore` but bound only part of
//! them to GDScript, so a script could call `store_32` yet not `store_var`, and
//! `FileAccess.get_open_error()` answered a *message string* where Godot answers
//! an `Error` code. That last one is the quiet half: scripts test it as
//! `if FileAccess.get_open_error() == ERR_FILE_NOT_FOUND`, and
//! `global_constants.rs:384` resolves that bareword to `Int(7)` — a String is
//! never equal to it, so the check silently took its else branch for *every*
//! cause.
//!
//! # What this asserts, and why in this shape
//!
//! ⚠️ The script writes with one handle and reads with a SECOND one, reopened.
//! Asserting on the same handle would let a buggy implementation pass by
//! returning values out of its own memory; the values have to survive a round
//! trip through the file.
//!
//! ⚠️ The project is booted with an explicit `user://` root.
//! `boot_project` leaves `user://` UNMAPPED (`project_boot.rs:91-95` — "Passing
//! `None` leaves `user://` unmapped"), and every `FileAccess.open("user://…")`
//! would then fail. A test written that way fails identically to one where the
//! binding is missing, which is the wrong signal entirely.
//!
//! ⚠️ Each script variable's declared default differs from the value written,
//! so no assertion here can pass on a variable the script never assigned.

use std::path::{Path, PathBuf};

use gdscene::{LifecycleManager, NodeId, SceneTree};
use gdvariant::Variant;
use patina_runner::project_boot::boot_project_with_user_root;

/// Godot's `ERR_FILE_NOT_FOUND`, the value `global_constants.rs:384` gives the
/// bareword a script would compare against.
const ERR_FILE_NOT_FOUND: i64 = 7;

fn fixture_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/fileaccess_io"
    ))
}

/// A private `user://` root for this test process. `std::env::temp_dir` rather
/// than a tempdir crate: patina-runner declares no dev-dependencies and this
/// test is not worth adding one.
fn user_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "patina_fileaccess_io_{}_{}",
        std::process::id(),
        line!()
    ));
    std::fs::create_dir_all(&dir).expect("user:// root must be creatable");
    dir
}

/// Boots the fixture exactly as `patina-runner <project_dir>` does, with
/// `user://` mapped, then runs `enter_tree` so `_ready` fires.
fn boot(user_dir: &Path) -> (SceneTree, NodeId) {
    let booted = boot_project_with_user_root(&fixture_dir(), Some(user_dir.to_path_buf()))
        .expect("the fixture project must boot");
    let mut tree = booted.tree;
    let main_root = booted
        .main_scene_root
        .expect("the project declares run/main_scene, so it must instance");
    LifecycleManager::enter_tree(&mut tree, main_root);
    (tree, main_root)
}

fn script_var(tree: &SceneTree, id: NodeId, name: &str) -> Variant {
    tree.get_script(id)
        .unwrap_or_else(|| panic!("the main scene must have its script attached to read `{name}`"))
        .get_property(name)
        .unwrap_or_else(|| panic!("the script must declare `{name}`"))
}

fn as_int(v: &Variant, name: &str) -> i64 {
    match v {
        Variant::Int(i) => *i,
        other => panic!("`{name}` should be an Int, got {other:?}"),
    }
}

fn as_float(v: &Variant, name: &str) -> f64 {
    match v {
        Variant::Float(f) => *f,
        Variant::Int(i) => *i as f64,
        other => panic!("`{name}` should be a number, got {other:?}"),
    }
}

/// **The acceptance test.**
#[test]
fn fileaccess_typed_io_script_roundtrip() {
    let user_dir = user_root();
    let (tree, main) = boot(&user_dir);
    let var = |n: &str| script_var(&tree, main, n);

    // -- fixture check first ------------------------------------------------
    //
    // Every assertion below reads a script variable. If `_ready` never ran, they
    // all read their declared defaults, which looks exactly like a binding that
    // returned the wrong value.
    assert_eq!(
        var("wrote"),
        Variant::Bool(true),
        "`_ready` must have run and completed the write phase, or nothing below \
         means anything. False means the script never attached or raised before \
         finishing — a raise here is itself the bug, since every method it calls \
         is supposed to be bound."
    );

    // -- the typed primitives round trip through the file -------------------
    assert_eq!(
        as_int(&var("r32"), "r32"),
        7,
        "`store_32(7)` then `get_32()` on a reopened handle must give 7 back"
    );
    assert_eq!(
        var("rline"),
        Variant::String("hi".to_string()),
        "`store_line(\"hi\")` then `get_line()` must give \"hi\" back, with the \
         newline consumed and not returned"
    );
    assert!(
        (as_float(&var("rdouble"), "rdouble") - 1.5).abs() < f64::EPSILON,
        "`store_double(1.5)` then `get_double()` must be exact; 1.5 is \
         representable, so any difference is a framing error rather than \
         rounding. Got {:?}",
        var("rdouble")
    );

    // -- store_var / get_var, the pair save games are written around --------
    //
    // ⚠️ Asserted through the DECOMPOSED dictionary rather than by comparing a
    // whole Variant, so a failure says which part of the structure was lost.
    // These were simply absent before: `store_var` had no arm at all, so the
    // script raised UndefinedFunction partway through `_ready`.
    assert_eq!(
        as_int(&var("rvar_keys"), "rvar_keys"),
        1,
        "the stored Dictionary must come back with its single key `a`"
    );
    assert_eq!(
        as_int(&var("rvar_a0"), "rvar_a0"),
        1,
        "the nested Array's Int element must survive the round trip"
    );
    assert!(
        (as_float(&var("rvar_a1"), "rvar_a1") - 2.5).abs() < f64::EPSILON,
        "the nested Array's Float element must survive the round trip; got {:?}",
        var("rvar_a1")
    );

    // -- the cursor ended where the data ended ------------------------------
    assert_eq!(
        var("at_eof"),
        Variant::Bool(true),
        "after reading everything that was written, `eof_reached()` must be \
         true. False means a read consumed fewer bytes than it stored — most \
         likely `get_var` not honouring the framed length."
    );

    // -- get_buffer is TYPED ------------------------------------------------
    //
    // ⚠️ The variant type is the assertion, not the contents. An `Array` of Int
    // carries the same numbers and would satisfy any length or value check,
    // while failing `var b: PackedByteArray = f.get_buffer(n)` in real script.
    match var("buf") {
        Variant::PackedByteArray(bytes) => assert_eq!(
            bytes.len(),
            4,
            "get_buffer(4) must return exactly the 4 bytes asked for"
        ),
        other => panic!(
            "`get_buffer` must return a PackedByteArray, got {other:?}. An \
             untyped Array is the pre-fix behaviour and cannot be assigned to a \
             PackedByteArray-typed variable."
        ),
    }

    // -- open() reports failure by returning null, not by raising -----------
    assert_eq!(
        var("missing_is_null"),
        Variant::Bool(true),
        "`FileAccess.open` on a missing file must return null so the script can \
         test it. Raising instead kills the script at the open, which is why \
         `var f = FileAccess.open(p, READ); if f == null:` could not be written."
    );
    assert_eq!(
        as_int(&var("open_err"), "open_err"),
        ERR_FILE_NOT_FOUND,
        "`FileAccess.get_open_error()` must answer the Godot Error CODE \
         ({ERR_FILE_NOT_FOUND} = ERR_FILE_NOT_FOUND), because that is what a \
         script compares it against. A message String here is never `==` to the \
         bareword, so the standard check silently took its else branch for \
         every cause."
    );

    // The control for the assertion above: a code that is *always* 7 would
    // satisfy it without tracking anything.
    assert_eq!(
        as_int(&var("ok_err"), "ok_err"),
        0,
        "after an open that SUCCEEDED, `get_open_error()` must be OK (0). A \
         non-zero value means the error is never cleared, which would make the \
         ERR_FILE_NOT_FOUND assertion above pass no matter what happened."
    );

    let _ = std::fs::remove_dir_all(&user_dir);
}
