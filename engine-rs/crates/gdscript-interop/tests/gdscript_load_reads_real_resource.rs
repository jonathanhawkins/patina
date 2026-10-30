//! pat-nzgdu.77 — `load()`/`preload()` return live text resources.
//!
//! Before this bead the `preload`/`load` builtin never touched the filesystem:
//! it returned a `ResourceRef` holding only the path plus a class name guessed
//! from the file extension, with `properties: HashMap::new()`. So
//! `load("res://x.tres").get("prop")` always answered `null` and every `.tres`
//! reported the generic class `Resource` regardless of its declared type.
//!
//! These tests drive the real interpreter dispatch (`run_class` ->
//! `instantiate_class` -> `call_instance_method`, the path the runner takes to
//! fire `_ready`/`_process`) against a `.tres` written to a temp project root,
//! proving the builtin parses the file rather than guessing at it.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// `res://` roots are process-global, so these tests must not point them at
/// their own temp dirs concurrently. Each test holds this for its duration.
static ROOT_LOCK: Mutex<()> = Mutex::new(());

fn lock_roots() -> MutexGuard<'static, ()> {
    ROOT_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

/// A `.tres` whose declared type and property values are all distinct from the
/// pre-fix placeholder's guesses (class `Resource`, no properties).
const SUB_TRES: &str = "\
[gd_resource type=\"Environment\" load_steps=1 format=3]

[resource]
fog_density = 0.75
fog_enabled = true
adjustment_brightness = 2
sky_name = \"dusk\"
";

/// Writes `SUB_TRES` into `dir` and points the `res://` root at it.
fn write_project(dir: &Path) {
    std::fs::write(dir.join("sub.tres"), SUB_TRES).expect("write sub.tres");
    gdcore::file_access::set_path_roots(Some(dir.to_path_buf()), None);
}

/// Parse `src` as a class, instantiate it, and call `method` with no args
/// through the real interpreter dispatch.
fn call_method(src: &str, method: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, &[])
        .unwrap_or_else(|e| panic!("call {method}() failed: {e:?}"))
}

/// The bead's headline acceptance: a GDScript running `load("res://sub.tres")`
/// reads real property values back out of the parsed file.
#[test]
fn gdscript_load_reads_real_resource() {
    let _guard = lock_roots();
    let tmp = tempfile::tempdir().expect("tempdir");
    write_project(tmp.path());

    let src = "\
extends Node
func check() -> Array:
    var res = load(\"res://sub.tres\")
    return [res.get(\"fog_density\"), res.get(\"fog_enabled\"), \
res.get(\"adjustment_brightness\"), res.get(\"sky_name\"), res.get_class()]
";
    let got = match call_method(src, "check") {
        Variant::Array(items) => items,
        other => panic!("expected Array, got {other:?}"),
    };

    // Each value must come from the parsed file, with its .tres type preserved
    // (Float/Bool/Int/String), not the placeholder's empty property map.
    assert_eq!(
        got.to_vec()[0],
        Variant::Float(0.75),
        "load() must return the .tres float property, not null",
    );
    assert_eq!(got.to_vec()[1], Variant::Bool(true), "bool property must parse");
    assert_eq!(got.to_vec()[2], Variant::Int(2), "int property must parse");
    assert_eq!(
        got.to_vec()[3],
        Variant::String("dusk".to_string()),
        "string property must parse",
    );
    assert_eq!(
        got.to_vec()[4],
        Variant::String("Environment".to_string()),
        "get_class() must report the .tres declared type, not the extension guess",
    );
}

/// `preload()` shares the builtin with `load()`, so it must read the file too.
#[test]
fn gdscript_preload_reads_real_resource() {
    let _guard = lock_roots();
    let tmp = tempfile::tempdir().expect("tempdir");
    write_project(tmp.path());

    let src = "\
extends Node
func check():
    return preload(\"res://sub.tres\").get(\"fog_density\")
";
    assert_eq!(
        call_method(src, "check"),
        Variant::Float(0.75),
        "preload() must parse the resource like load()",
    );
}

/// A missing file must not halt the script: Godot's `load()` of an absent path
/// reports an error and yields a null-ish value rather than aborting. The
/// builtin keeps returning its path-only placeholder, so `get()` answers null
/// and the script keeps running.
#[test]
fn gdscript_load_missing_resource_does_not_error() {
    let _guard = lock_roots();
    let tmp = tempfile::tempdir().expect("tempdir");
    write_project(tmp.path());

    let src = "\
extends Node
func check():
    return load(\"res://nope.tres\").get(\"fog_density\")
";
    assert_eq!(
        call_method(src, "check"),
        Variant::Nil,
        "missing .tres must degrade to a null property, not a runtime error",
    );
}

/// A malformed `.tres` must degrade the same way rather than propagating a
/// parse error out of the builtin.
#[test]
fn gdscript_load_malformed_resource_does_not_error() {
    let _guard = lock_roots();
    let tmp = tempfile::tempdir().expect("tempdir");
    write_project(tmp.path());
    std::fs::write(tmp.path().join("bad.tres"), "fog_density = 0.5\n").expect("write bad.tres");

    let src = "\
extends Node
func check() -> Array:
    var res = load(\"res://bad.tres\")
    return [res.get(\"fog_density\"), res.get_path()]
";
    let got = match call_method(src, "check") {
        Variant::Array(items) => items,
        other => panic!("expected Array, got {other:?}"),
    };
    assert_eq!(got.to_vec()[0], Variant::Nil, "malformed .tres must not yield values");
    assert_eq!(
        got.to_vec()[1],
        Variant::String("res://bad.tres".to_string()),
        "the placeholder must still carry the requested path",
    );
}
