//! pat-s3rrv.123 — `EditorInterface.get_editor_paths()` and
//! `get_command_palette()`.
//!
//! `call_editor_interface_static` had exactly ONE arm (`get_resource_filesystem`);
//! every other member of Godot's 70 raised `UndefinedFunction`.
//!
//! 🔑 **The implementations already existed — only the documented spelling was
//! missing.** Patina exposes `EditorPaths` and `EditorCommandPalette` as bare
//! globals, which Godot does not have: upstream reaches both *through*
//! `EditorInterface`. So `EditorPaths.get_config_dir()` worked while
//! `EditorInterface.get_editor_paths().get_config_dir()` — the spelling every
//! Godot `@tool` script and every piece of documentation uses — did not.
//!
//! Each test therefore asserts the two spellings return the **same value**,
//! rather than asserting a hardcoded path: that pins the wiring without
//! re-testing the underlying implementation, and it cannot drift if the paths
//! change.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn eval(src: &str) -> Variant {
    let probe = format!("extends Node\nfunc probe():\n\x20   return {src}\n");
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&probe).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "probe", &[])
        .unwrap_or_else(|e| panic!("`{src}` raised: {e:?}"))
}

/// **The core of the slice.** The accessor exists and reaches the same
/// implementation the bare global does.
#[test]
fn get_editor_paths_reaches_the_same_implementation_as_the_bare_global() {
    for method in [
        "get_data_dir",
        "get_config_dir",
        "get_cache_dir",
        "get_project_settings_dir",
        "is_self_contained",
        "get_self_contained_file",
    ] {
        let through_interface = eval(&format!("EditorInterface.get_editor_paths().{method}()"));
        let bare = eval(&format!("EditorPaths.{method}()"));
        assert_eq!(
            through_interface, bare,
            "EditorInterface.get_editor_paths().{method}() must reach the same \
             implementation as the bare EditorPaths.{method}()"
        );
    }
}

/// The handle is a real object id, not `Nil` — a `Nil` would make the method
/// calls above fail in a way that looks like the accessor working.
#[test]
fn the_accessors_return_object_handles() {
    assert!(
        matches!(
            eval("EditorInterface.get_editor_paths()"),
            Variant::ObjectId(_)
        ),
        "get_editor_paths() must hand back an object"
    );
    assert!(
        matches!(
            eval("EditorInterface.get_command_palette()"),
            Variant::ObjectId(_)
        ),
        "get_command_palette() must hand back an object"
    );
}

/// ⚠️ The two handles must be **distinguishable**. They are told apart by their
/// top tag byte, and handle tags are a workspace-wide namespace — a duplicated
/// tag would silently route one accessor's methods into the other's dispatcher,
/// which is the failure this guards.
#[test]
fn the_two_handles_are_distinct() {
    let paths = eval("EditorInterface.get_editor_paths()");
    let palette = eval("EditorInterface.get_command_palette()");
    assert_ne!(
        paths, palette,
        "the paths and palette handles must not share an id"
    );

    // And each routes to its OWN surface. `add_command` belongs to the palette
    // and is not a paths method, so a shared dispatcher would raise here —
    // `eval` panics on a `RuntimeError`, which IS the assertion.
    eval("EditorInterface.get_command_palette().add_command(\"n\", \"k\", \"c\")");
    // Conversely a paths method on the palette handle must NOT resolve, or the
    // two tags are routing to one dispatcher.
    let mut interp = Interpreter::new();
    let probe = "extends Node\nfunc probe():\n\x20   return EditorInterface.get_command_palette().get_config_dir()\n";
    let class_def = interp.run_class(probe).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    assert!(
        interp
            .call_instance_method(&mut inst, "probe", &[])
            .is_err(),
        "get_config_dir is an EditorPaths method; the palette handle must not \
         answer it"
    );
}

/// The control: an unimplemented member still reports itself as missing rather
/// than answering something plausible. 69 of Godot's 70 members are still
/// absent, and they must fail loudly.
#[test]
fn an_unimplemented_member_still_raises() {
    let probe =
        "extends Node\nfunc probe():\n\x20   return EditorInterface.get_edited_scene_root()\n";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(probe).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    assert!(
        interp
            .call_instance_method(&mut inst, "probe", &[])
            .is_err(),
        "get_edited_scene_root is not implemented yet and must not answer a \
         plausible-looking value"
    );
}
