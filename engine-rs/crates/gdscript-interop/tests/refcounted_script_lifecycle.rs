//! pat-0q2kx.163 — `RefCounted` reachable from GDScript: construction and the
//! EXPLICIT reference-count API.
//!
//! Before this, `RefCounted` was registered in ClassDB and
//! `gdobject::ref_counted::RefCountedBase` implemented Godot's counting
//! semantics correctly — but the interpreter's `.new()` gate had arms only for
//! `ConfigFile`, `XMLParser`, `ZIPReader`, `ZIPPacker`, `PCKPacker`,
//! `CameraFeed`, `WebSocketPeer` and `UndoRedo`. A script could not construct a
//! `RefCounted` at all, so it could never observe a reference count, and
//! `weakref` had nothing counted to point at.
//!
//! ⚠️ SCOPE — this file deliberately does NOT test auto-free on scope exit, and
//! the bead is NOT complete. Godot also drops a reference when a binding leaves
//! scope; here an object lives until a script calls `unreference()`. Wiring the
//! implicit half means refcounting the interpreter's `Environment`, and
//! `impl Clone for Interpreter` copies the whole environment without
//! incrementing — so a naive `pop_scope` hook either never frees or frees
//! objects other scripts still hold. That needs a build-capable lane; see the
//! bead notes. The acceptance test `refcounted_weakref_lifecycle` covers the
//! implicit half and is intentionally still absent.
//!
//! Integration tests rather than unit tests because `-p gdscript-interop --lib`
//! cannot pass — a pre-existing `fuzz_deeply_nested_parens` stack overflow
//! cancels that run — so `--lib` tests here would be unverifiable.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `src` and returns its `return` value.
fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

fn arr(v: &Variant) -> &[Variant] {
    match v {
        Variant::Array(items) => items,
        other => panic!("expected an array, got {other:?}"),
    }
}

fn int_at(v: &Variant, i: usize) -> i64 {
    match &arr(v)[i] {
        Variant::Int(n) => *n,
        other => panic!("expected an int at {i}, got {other:?}"),
    }
}

/// A script can construct a `RefCounted`, and it starts at 1 — the first clause
/// of the bead's acceptance.
#[test]
fn refcounted_new_starts_with_one_reference() {
    let v = run_val("var r = RefCounted.new()\nreturn r.get_reference_count()\n");
    assert_eq!(
        v,
        Variant::Int(1),
        "Godot's RefCounted.new() yields a count of 1 — creating the object is \
         itself the first reference"
    );
}

/// Taking a second reference and releasing one leaves the object alive with the
/// first still held. This is the acceptance's "assigning to a second var then
/// releasing one keeps it alive", expressed with the EXPLICIT API since the
/// implicit scope half is not implemented.
#[test]
fn a_second_reference_survives_one_release() {
    let v = run_val(
        "var r = RefCounted.new()\n\
         r.reference()\n\
         var two = r.get_reference_count()\n\
         var freed = r.unreference()\n\
         var one = r.get_reference_count()\n\
         return [two, one, freed]\n",
    );
    assert_eq!(int_at(&v, 0), 2, "reference() takes a second reference");
    assert_eq!(int_at(&v, 1), 1, "releasing one of two leaves one held");
    assert_eq!(
        arr(&v)[2],
        Variant::Bool(false),
        "unreference() reports whether it FREED the object; with a reference \
         still held it must be false"
    );
}

/// The last release frees, and the object then reads as invalid — which is what
/// `weakref`/`is_instance_valid` consult.
#[test]
fn the_last_release_frees_and_invalidates() {
    let v = run_val(
        "var r = RefCounted.new()\n\
         var freed = r.unreference()\n\
         return [freed, is_instance_valid(r)]\n",
    );
    assert_eq!(
        arr(&v)[0],
        Variant::Bool(true),
        "dropping the only reference frees the object and says so"
    );
    assert_eq!(
        arr(&v)[1],
        Variant::Bool(false),
        "a freed RefCounted must stop being a valid instance — release \
         unregisters it from the same alive-objects registry weakref reads"
    );
}

/// `weakref(r).get_ref()` follows the object's life: something while held,
/// nothing once released. The weakref arm and `WeakRef::get_ref` both already
/// existed; until `RefCounted.new()` there was simply nothing counted to aim
/// them at.
#[test]
fn a_weakref_tracks_a_refcounted_across_its_release() {
    let v = run_val(
        "var r = RefCounted.new()\n\
         var w = weakref(r)\n\
         var before = w.get_ref() != null\n\
         r.unreference()\n\
         var after = w.get_ref() != null\n\
         return [before, after]\n",
    );
    assert_eq!(
        arr(&v)[0],
        Variant::Bool(true),
        "a weakref over a held object resolves"
    );
    assert_eq!(
        arr(&v)[1],
        Variant::Bool(false),
        "and reports gone after the last reference is dropped"
    );
}

/// Two `.new()` calls are two objects: releasing one must not disturb the other.
/// A registry keyed carelessly (or a shared handle) would fail here while every
/// single-object test above still passed.
#[test]
fn separate_instances_are_independent() {
    let v = run_val(
        "var a = RefCounted.new()\n\
         var b = RefCounted.new()\n\
         a.unreference()\n\
         return [is_instance_valid(a), is_instance_valid(b), b.get_reference_count()]\n",
    );
    assert_eq!(arr(&v)[0], Variant::Bool(false), "a was released");
    assert_eq!(
        arr(&v)[1],
        Variant::Bool(true),
        "b is a different object and must be untouched"
    );
    assert_eq!(int_at(&v, 2), 1, "b still holds its own single reference");
}

/// `init_ref()` reports that a reference is held without taking another. It is
/// not a synonym for `reference()`, and treating it as one would inflate every
/// count by an object's lifetime.
#[test]
fn init_ref_does_not_increment() {
    let v = run_val(
        "var r = RefCounted.new()\n\
         var held = r.init_ref()\n\
         return [held, r.get_reference_count()]\n",
    );
    assert_eq!(arr(&v)[0], Variant::Bool(true));
    assert_eq!(
        int_at(&v, 1),
        1,
        "init_ref must not take a second reference"
    );
}

/// The handle answers `Object`-level identity too, so it can be passed to the
/// same helpers any other engine object is.
#[test]
fn refcounted_reports_its_class() {
    let v = run_val(
        "var r = RefCounted.new()\n\
         return [r.get_class(), r.is_class(\"RefCounted\"), r.is_class(\"Node\")]\n",
    );
    assert_eq!(arr(&v)[0], Variant::String("RefCounted".into()));
    assert_eq!(arr(&v)[1], Variant::Bool(true));
    assert_eq!(
        arr(&v)[2],
        Variant::Bool(false),
        "a RefCounted is not a Node — an is_class that answered true to \
         everything would make the dispatch chain meaningless"
    );
}
