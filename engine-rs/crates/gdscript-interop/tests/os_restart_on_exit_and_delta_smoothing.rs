//! pat-s3rrv.108 — `OS.set_restart_on_exit()` + readers, and the
//! `delta_smoothing` pair.
//!
//! Both were absent, so the calls raised `UndefinedFunction`. `set_restart_on_exit`
//! is how a project applies a setting change that needs a relaunch.
//!
//! ⚠️ ONE test function on purpose. Both features are backed by **process-wide**
//! state in `gdplatform::os` (as Godot's are — they live on the OS singleton),
//! and cargo runs a binary's tests on parallel threads. Split across tests, one
//! asserting the default while another had just flipped it would fail
//! intermittently.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

const SRC: &str = "extends Node\n\
                   \n\
                   func smoothing_default():\n\
                   \x20   return OS.is_delta_smoothing_enabled()\n\
                   \n\
                   func smoothing_off():\n\
                   \x20   OS.set_delta_smoothing(false)\n\
                   \x20   return OS.is_delta_smoothing_enabled()\n\
                   \n\
                   func restart_default():\n\
                   \x20   return OS.is_restart_on_exit_set()\n\
                   \n\
                   func arm_restart():\n\
                   \x20   OS.set_restart_on_exit(true, PackedStringArray([\"--fullscreen\"]))\n\
                   \x20   return OS.is_restart_on_exit_set()\n\
                   \n\
                   func restart_args():\n\
                   \x20   return OS.get_restart_on_exit_arguments()\n\
                   \n\
                   func disarm_restart():\n\
                   \x20   OS.set_restart_on_exit(false)\n\
                   \x20   return OS.get_restart_on_exit_arguments()\n";

fn call(method: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(SRC).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, &[])
        .unwrap_or_else(|e| panic!("OS.{method} raised: {e:?}"))
}

#[test]
fn restart_on_exit_and_delta_smoothing_round_trip() {
    // 🔑 Delta smoothing defaults to TRUE (`OS.xml:898`). "Off" is the tempting
    // default for an engine that does not implement the smoothing yet, and it
    // would report the opposite of Godot for every fresh project.
    assert_eq!(
        call("smoothing_default"),
        Variant::Bool(true),
        "OS.delta_smoothing defaults to true in Godot; false here means the \
         default was chosen rather than read from the docs"
    );
    assert_eq!(
        call("smoothing_off"),
        Variant::Bool(false),
        "set_delta_smoothing(false) must be observable"
    );

    // Restart-on-exit starts disarmed.
    assert_eq!(
        call("restart_default"),
        Variant::Bool(false),
        "a project does not relaunch itself unless asked"
    );
    assert_eq!(
        call("arm_restart"),
        Variant::Bool(true),
        "set_restart_on_exit(true, ...) must arm the relaunch"
    );
    assert_eq!(
        call("restart_args"),
        Variant::PackedStringArray(vec!["--fullscreen".to_string()]),
        "the stored command line must come back through \
         get_restart_on_exit_arguments()"
    );

    // 🔑 Disarming with the default (empty) argument list also CLEARS the
    // arguments, because Godot replaces both halves on every call.
    //
    // ⚠️ Keeping the old arguments would look harmless and then relaunch a
    // later `set_restart_on_exit(true)` with a command line the caller never
    // asked for — a bug that only shows up on the second arming.
    assert_eq!(
        call("disarm_restart"),
        Variant::PackedStringArray(Vec::new()),
        "set_restart_on_exit(false) replaces BOTH halves, so the arguments are \
         cleared rather than left primed for a later arming"
    );
}
