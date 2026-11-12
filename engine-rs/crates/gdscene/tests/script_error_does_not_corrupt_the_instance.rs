//! pat-s3rrv.19 — a raised script error must not corrupt the instance.
//!
//! `call_instance_method_impl` and `call_user_func_impl` both ran their body as
//! `self.exec_stmt(&body[i])?`. The `?` returned **before every line of the
//! teardown** that the normal exit performs a few lines below:
//!
//! * `instance.properties = updated.properties.clone()` — so every property the
//!   method had already assigned was silently discarded. Godot keeps them.
//! * two `pop_scope()` calls — so the function's scopes leaked for the life of
//!   the instance.
//! * `self_instance = prev_self` — so the errored instance stayed installed.
//! * `call_depth -= 1` — so the recursion budget shrank with every caught error.
//!
//! ⚠️ The leaked scope is the subtle one, and it is what made this bead
//! undiagnosable for two passes. A bare `count = 1` inside a method writes to
//! the *instance property* only if the name is not shadowed, and
//! `is_shadowed_in_inner_scope` answers "shadowed" when the name appears in two
//! scopes at once. Every call defines the instance's properties into its own
//! scope — so after one leak the name is present twice, and from then on every
//! bare assignment in *any* later call silently wrote to the dead scope
//! instead. The symptom is a script that runs to completion, raises nothing,
//! and leaves its class vars at their initialisers.

use gdscene::node::NodeId;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Reads a class var off the instance. Script vars and node properties are
/// separate stores, so this reads the script's own.
fn var_of(script: &GDScriptNodeInstance, name: &str) -> Variant {
    script
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

fn int_of(script: &GDScriptNodeInstance, name: &str) -> i64 {
    match var_of(script, name) {
        Variant::Int(i) => i,
        other => panic!("`{name}` is not an Int: {other:?}"),
    }
}

const SRC: &str = "\
extends Node
var stage = 0
var after = 0
func boom():
    stage = 1
    stage = 2
    var x = no_such_function()
    stage = 99
func bump():
    after = 7
    stage = stage + 10
";

/// **The acceptance test**: writes made before the error survive it, and a
/// later call still reaches the instance.
#[test]
fn script_error_does_not_corrupt_the_instance() {
    let mut script = GDScriptNodeInstance::from_source(SRC, NodeId::next()).expect("parses");

    let err = script.call_method("boom", &[]);
    assert!(
        err.is_err(),
        "`no_such_function()` must raise — if it does not, this test is not \
         exercising the error path at all",
    );

    assert_eq!(
        int_of(&script, "stage"),
        2,
        "the two assignments that ran BEFORE the raise must survive it. 0 means \
         the whole method's property writes were rolled back, which is what \
         made a `stage` progress counter useless for diagnosis: it read its \
         initialiser however far the method actually got",
    );
    assert_ne!(
        int_of(&script, "stage"),
        99,
        "…and the assignment AFTER the raise must not have run",
    );

    // -- the leak: a LATER call must still write to the instance -------------
    let ok = script.call_method("bump", &[]);
    assert!(ok.is_ok(), "`bump` does not raise: {ok:?}");
    assert_eq!(
        int_of(&script, "after"),
        7,
        "a bare assignment in a call made AFTER an error must still reach the \
         instance property. The failing scope leak left `after` defined in a \
         dead scope as well as the live one, so `is_shadowed_in_inner_scope` \
         reported it shadowed and the write went to the leaked scope — the \
         call succeeded and the class var never changed",
    );
    assert_eq!(
        int_of(&script, "stage"),
        12,
        "…and a read-modify-write must see the value the errored call left (2), \
         not the initialiser",
    );
}

/// The same teardown on a plain function, not just a method (pat-s3rrv.19).
///
/// `call_user_func_impl` had the identical `?` and leaked its scope,
/// `call_depth`, `current_function` and the call-stack frame. A leaked
/// `call_depth` is the nastiest of those: it never resets, so enough caught
/// errors eventually trip the recursion limit in an unrelated call.
#[test]
fn a_caught_error_does_not_consume_the_recursion_budget() {
    const SRC: &str = "\
extends Node
var depth_ok = 0
func recurse(n):
    if n <= 0:
        return 0
    return 1 + recurse(n - 1)
func boom():
    var x = no_such_function()
func run():
    self.depth_ok = recurse(20)
";
    let mut script = GDScriptNodeInstance::from_source(SRC, NodeId::next()).expect("parses");

    // Raise more times than the recursion limit allows (MAX_RECURSION_DEPTH is
    // 64). Each raise used to leak a `call_depth` increment that was never
    // given back, so past the limit EVERY later call fails — the count is
    // chosen so the failure does not depend on how deep `recurse` goes.
    for _ in 0..70 {
        assert!(script.call_method("boom", &[]).is_err(), "boom must raise");
    }

    let ran = script.call_method("run", &[]);
    assert!(
        ran.is_ok(),
        "70 caught errors must leave the recursion budget untouched — a \
         MaxRecursionDepth here means each raise permanently consumed a frame: \
         {ran:?}",
    );
    assert_eq!(
        int_of(&script, "depth_ok"),
        20,
        "…and the recursion must return its real depth, not the initialiser",
    );
}
