//! pat-s3rrv.108 — the `OS` permission quartet and `OS.get_version_alias()`.
//!
//! All five raised `UndefinedFunction` before this, so a cross-platform script
//! that guards an Android path could not even *ask*.
//!
//! 🔑 **The two permission answers deliberately disagree**, and that is the whole
//! subtlety of the group. `request_permission()` reports `true` while
//! `get_granted_permissions()` reports an empty list. Deriving either from the
//! other is the obvious implementation and it is wrong in both directions — see
//! the test comments.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

const SRC: &str = "extends Node\n\
                   \n\
                   func ask_one():\n\
                   \x20   return OS.request_permission(\"android.permission.RECORD_AUDIO\")\n\
                   \n\
                   func ask_all():\n\
                   \x20   return OS.request_permissions()\n\
                   \n\
                   func granted():\n\
                   \x20   return OS.get_granted_permissions()\n\
                   \n\
                   func revoke():\n\
                   \x20   OS.revoke_granted_permissions()\n\
                   \x20   return OS.get_granted_permissions()\n\
                   \n\
                   func alias():\n\
                   \x20   return OS.get_version_alias()\n\
                   \n\
                   func version():\n\
                   \x20   return OS.get_version()\n";

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
fn permission_requests_are_granted_by_default() {
    // 🔑 `true`, not `false`. "No permission system, so nothing is granted" is
    // the intuitive reading and it inverts Godot: `core/os/os.h:349-350` has the
    // base class `return true`, because a platform that never withholds a
    // permission has already granted it. A `false` here sends every
    // `if OS.request_permission(...)` guard down its denied branch on desktop.
    assert_eq!(
        call("ask_one"),
        Variant::Bool(true),
        "Godot's base request_permission() grants; only Android gates"
    );
    assert_eq!(
        call("ask_all"),
        Variant::Bool(true),
        "request_permissions() follows the same default (core/os/os.h:350)"
    );
}

#[test]
fn granted_permission_list_is_empty_despite_requests_succeeding() {
    // 🔑 The discriminating case. This list is NOT "what is permitted": it is the
    // set of dangerous permissions granted at runtime (Android) or the folders a
    // sandboxed macOS app was handed. A desktop build accumulates neither, so it
    // is empty *while* every request above still answers `true`.
    //
    // ⚠️ An implementation that keeps the two consistent is wrong whichever way
    // it goes — returning the requested name here would invent an Android
    // grant record, and returning `false` above would deny a permitted call.
    assert_eq!(
        call("granted"),
        Variant::PackedStringArray(Vec::new()),
        "no runtime grant records exist off Android/sandboxed-macOS"
    );

    // Revoking is a no-op here rather than an error, and must not disturb the
    // list. This is the zero-assertion's positive control: the same call is
    // proven reachable and non-erroring by the assertions above, so an empty
    // result here means "cleared nothing", not "never ran".
    assert_eq!(
        call("revoke"),
        Variant::PackedStringArray(Vec::new()),
        "revoke_granted_permissions() clears the sandbox folder list; there is \
         nothing to clear here, and it must not raise"
    );
}

#[test]
fn version_alias_reports_the_os_version_not_the_engine_version() {
    let alias = call("alias");
    let version = call("version");

    // ⚠️ The tempting wrong answer is the *engine* version (4.x). Godot's
    // `get_version_alias()` is the branded **operating system** version, and its
    // base implementation returns `get_version()` verbatim
    // (`core/os/os.h:220`); only Windows/macOS/Android substitute a marketing
    // name. Pinning the relation catches a swap to the engine version.
    assert_eq!(
        alias, version,
        "get_version_alias() defaults to get_version() (core/os/os.h:220)"
    );

    // Positive control for the assertion above, which would also hold if both
    // sides were empty. On the platforms `gdplatform::os::version()` implements,
    // the host version is a real non-empty string.
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        let Variant::String(s) = &alias else {
            panic!("get_version_alias() must return a String, got {alias:?}");
        };
        assert!(
            !s.is_empty(),
            "the host OS version is discoverable on this platform, so an empty \
             alias means version() failed rather than agreed"
        );
    }
}
