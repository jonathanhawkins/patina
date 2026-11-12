//! pat-s3rrv.217 — `Thread.set_thread_safety_checks_enabled()` reaches the
//! engine flag from GDScript.
//!
//! `gdplatform::thread::set_thread_safety_checks_enabled` (and its reader) were
//! implemented and unit-tested, and no script could call either: the `Thread`
//! dispatch covered `start` / `get_id` / `is_started` / `is_alive` /
//! `wait_to_finish` / `is_main_thread` only, and all six live in the **instance**
//! table reached through an `ObjectId`. `Thread.xml` marks
//! `set_thread_safety_checks_enabled` and `is_main_thread`
//! `qualifiers="static"`, so a script spells them on the bare class name with no
//! instance — a spelling that had no arm at all.
//!
//! ## Why the assertions read the flag rather than the call's value
//!
//! The method returns `void`, so a dispatch arm that accepted the call and did
//! nothing would be indistinguishable from a correct one if the test only
//! checked that the call succeeded. Every assertion below therefore reads
//! `is_thread_safety_checks_enabled()` back, and each one is paired with the
//! opposite state so a flag that never moves cannot pass.
//!
//! The flag is **thread-local** and a script body runs inline on the calling
//! thread, so the value these tests observe is the one the script set.

use gdobject::class_db::{class_has_method, get_class_info};
use gdobject::register_3d_classes;
use gdplatform::thread::{is_thread_safety_checks_enabled, set_thread_safety_checks_enabled};
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `method` on a fresh instance of `src` through the real VM dispatch.
///
/// Copied from `threading.rs`, which pins this harness for the same classes.
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

const SRC: &str = "\
extends Node

func disable():
    Thread.set_thread_safety_checks_enabled(false)
    return 1

func enable():
    Thread.set_thread_safety_checks_enabled(true)
    return 1

func disable_via_instance():
    var t = Thread.new()
    t.set_thread_safety_checks_enabled(false)
    return 1

func main_thread():
    return Thread.is_main_thread()

func construct():
    var t = Thread.new()
    return t.is_started()
";

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** A script calling the static method moves the
/// engine's thread-local flag, in both directions.
#[test]
fn script_static_call_toggles_thread_safety_checks() {
    // The control: the documented default, before any script has run. If this
    // ever fails the rest of the test proves nothing, because the "after"
    // values below would already be in place.
    assert!(
        is_thread_safety_checks_enabled(),
        "checks default to enabled on every thread"
    );

    assert_eq!(call_method(SRC, "disable"), Variant::Int(1));
    assert!(
        !is_thread_safety_checks_enabled(),
        "Thread.set_thread_safety_checks_enabled(false) must reach the engine flag"
    );

    // The other direction, so an arm that unconditionally writes `false` — or
    // one that ignores its argument entirely — fails here.
    assert_eq!(call_method(SRC, "enable"), Variant::Int(1));
    assert!(
        is_thread_safety_checks_enabled(),
        "passing true must switch the checks back on"
    );
}

/// A static may be called through an instance in Godot, so both spellings must
/// land on the same flag.
#[test]
fn instance_spelling_toggles_the_same_flag() {
    set_thread_safety_checks_enabled(true);
    assert!(is_thread_safety_checks_enabled(), "control: starts enabled");

    assert_eq!(call_method(SRC, "disable_via_instance"), Variant::Int(1));
    assert!(
        !is_thread_safety_checks_enabled(),
        "t.set_thread_safety_checks_enabled(false) must reach the same flag as the class spelling"
    );

    set_thread_safety_checks_enabled(true);
}

/// `is_main_thread` is static in `Thread.xml` too, and the class-name spelling
/// had no arm either.
#[test]
fn is_main_thread_answers_on_the_class_name() {
    assert_eq!(call_method(SRC, "main_thread"), Variant::Bool(true));
}

/// **Regression guard.** The static arm intercepts member calls on the `Thread`
/// identifier, which is the same identifier `Thread.new()` is spelled on —
/// unrecognised members must fall through to the constructor, or wiring the
/// static method would break every existing threading script.
#[test]
fn thread_new_still_constructs() {
    assert_eq!(
        call_method(SRC, "construct"),
        Variant::Bool(false),
        "Thread.new() must still build a handle whose is_started() is false"
    );
}

/// ClassDB must agree, or `has_method()` and the inspector deny a method the
/// interpreter dispatches.
#[test]
fn set_thread_safety_checks_enabled_is_registered() {
    register_3d_classes();

    assert!(
        class_has_method("Thread", "set_thread_safety_checks_enabled"),
        "Thread.set_thread_safety_checks_enabled must be registered"
    );

    let info = get_class_info("Thread").expect("Thread is registered");
    let method = info
        .methods
        .iter()
        .find(|m| m.name == "set_thread_safety_checks_enabled")
        .expect("method present in the registration");
    assert!(
        method.is_static,
        "Thread.xml marks it qualifiers=\"static\""
    );
    assert_eq!(
        method.argument_count, 1,
        "one bool parameter, per Thread.xml"
    );
}
