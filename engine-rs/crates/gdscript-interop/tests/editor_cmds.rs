//! pat-0q2kx.241 — `EditorCommandPalette` reached from GDScript.
//!
//! The bead's acceptance test lives in `gdeditor` because it must also assert
//! on the served editor's `/api/commands` endpoint. These tests cover the other
//! side of the seam — that a script can actually *call* the surface — without
//! needing an HTTP server or a populated ClassDB, so a failure here points at
//! the interpreter rather than at the editor.
//!
//! Binary named `editor_cmds` to keep the verify command inside the 58-char
//! truncation limit.

use gdscript_interop::editor_commands;
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `check()` on a script and returns its value.
///
/// The command store is process-global, so it is cleared first. Under nextest
/// each test gets its own process, which is what makes that safe.
fn call_check(src: &str) -> Variant {
    editor_commands::clear();
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() failed: {e:?}"))
}

/// A `@tool` plugin's registration reaches the store with every field intact.
#[test]
fn add_command_from_script_reaches_the_store() {
    let src = "\
@tool
extends EditorPlugin

func _on_hello():
    pass

func check():
    EditorCommandPalette.add_command(\"Say Hello\", \"demo/hello\", _on_hello, \"Ctrl+Alt+H\")
    return EditorCommandPalette.get_command_count()
";
    assert_eq!(call_check(src), Variant::Int(1));

    let cmd = editor_commands::find("demo/hello").expect("the command must be registered");
    assert_eq!(cmd.command_name, "Say Hello");
    assert_eq!(cmd.shortcut.as_deref(), Some("Ctrl+Alt+H"));
    assert_eq!(
        cmd.callback.as_deref(),
        Some("_on_hello"),
        "a bare method reference must record the method it would invoke",
    );
    assert_eq!(cmd.category(), "demo", "category comes from the key prefix");
}

/// `has_command` and `remove_command` answer from the same store.
#[test]
fn has_and_remove_round_trip() {
    let src = "\
@tool
extends EditorPlugin

func check():
    EditorCommandPalette.add_command(\"A\", \"demo/a\", null, \"None\")
    var had = EditorCommandPalette.has_command(\"demo/a\")
    var removed = EditorCommandPalette.remove_command(\"demo/a\")
    var again = EditorCommandPalette.remove_command(\"demo/a\")
    return [had, removed, again, EditorCommandPalette.has_command(\"demo/a\")]
";
    match call_check(src) {
        Variant::Array(v) => {
            assert_eq!(v[0], Variant::Bool(true), "the command must be found");
            assert_eq!(v[1], Variant::Bool(true), "removing it reports success");
            assert_eq!(
                v[2],
                Variant::Bool(false),
                "removing a second time is a no-op, not a success",
            );
            assert_eq!(v[3], Variant::Bool(false));
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// Godot spells "no shortcut" as the literal string `"None"`. It must not
/// become a shortcut of that name.
#[test]
fn the_none_shortcut_is_normalised_away() {
    let src = "\
@tool
extends EditorPlugin

func check():
    EditorCommandPalette.add_command(\"A\", \"demo/a\", null, \"None\")
    return 0
";
    call_check(src);
    assert_eq!(editor_commands::find("demo/a").unwrap().shortcut, None);
}

/// Re-registering a key replaces the entry. A plugin that is disabled and
/// re-enabled runs `_enter_tree` again, and Godot's palette shows one entry per
/// key rather than one per enable.
#[test]
fn re_registering_replaces_instead_of_duplicating() {
    let src = "\
@tool
extends EditorPlugin

func check():
    EditorCommandPalette.add_command(\"First\", \"demo/x\", null, \"None\")
    EditorCommandPalette.add_command(\"Second\", \"demo/x\", null, \"None\")
    return EditorCommandPalette.get_command_count()
";
    assert_eq!(
        call_check(src),
        Variant::Int(1),
        "a re-enabled plugin must not double its palette entry",
    );
    assert_eq!(
        editor_commands::find("demo/x").unwrap().command_name,
        "Second",
    );
}

/// A key with no `/` still gets a usable category rather than an empty one.
#[test]
fn a_bare_key_gets_the_plugin_category() {
    let src = "\
@tool
extends EditorPlugin

func check():
    EditorCommandPalette.add_command(\"Bare\", \"bare\", null, \"None\")
    return 0
";
    call_check(src);
    assert_eq!(editor_commands::find("bare").unwrap().category(), "plugin");
}

/// An unknown method is a clear error, not a silent no-op — otherwise a typo in
/// a plugin script would look like a working registration.
#[test]
fn an_unknown_method_errors() {
    let mut interp = Interpreter::new();
    let src = "\
@tool
extends EditorPlugin

func check():
    return EditorCommandPalette.no_such_method()
";
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    let err = interp.call_instance_method(&mut inst, "check", &[]);
    assert!(
        err.is_err(),
        "calling an unsupported palette method must raise, got {err:?}",
    );
}
