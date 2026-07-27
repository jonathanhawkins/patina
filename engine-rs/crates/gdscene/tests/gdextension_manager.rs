//! pat-0q2kx.252 — the `GDExtensionManager` singleton reachable from GDScript.
//!
//! Godot ground truth: `doc/classes/GDExtensionManager.xml` over
//! `core/extension/gdextension_manager.{h,cpp}`.
//!
//! **What this slice does and does not do.** Patina *recognises* a
//! `.gdextension` manifest — it parses the file, picks the library for the
//! current platform, and records that path plus the entry symbol so a script can
//! introspect the registry. It does **not** `dlopen` anything and never runs a
//! native entry point. The scope decision is recorded in
//! `THIRDPARTY_STRATEGY.md`. The statuses below are the honest consequence of
//! that: recognised → `OK`, unreadable/no-library-for-this-platform → `FAILED`,
//! reload → `NEEDS_RESTART` (a reload can never take effect in-process when
//! nothing is executing).
//!
//! ⚠️ **The bead's acceptance text has a wrong number.** It says a missing
//! manifest returns "LOAD_STATUS_FAILED (2)". Godot's `LoadStatus` is a plain
//! C++ enum numbering from **0** (`gdextension_manager.h:54-58`):
//! `OK=0, FAILED=1, ALREADY_LOADED=2, NOT_LOADED=3, NEEDS_RESTART=4`. So
//! `FAILED` is **1**, and 2 is `ALREADY_LOADED` — a different outcome entirely.
//! This test asserts Godot's values, and the assertion below covers both, so a
//! confusion between the two cannot pass.
//!
//! ⚠️ The registry is a **process-global** (`GDExtensionManager` is a process
//! singleton in Godot too, and a script reaches it holding no handle). Every
//! host-mutating assertion therefore lives in the single acceptance test, after
//! one `reset_host()`, so the file is correct under thread-per-test `cargo test`
//! as well as process-per-test `nextest`.

use gdcore::gdextension_host as host;
use gdscene::node::{Node, NodeId};
use gdscene::{GDScriptNodeInstance, SceneTree};
use gdvariant::Variant;

/// The manifest path a script names — what `get_loaded_extensions()` reports.
const MANIFEST_PATH: &str = "res://addons/demo/demo.gdextension";

/// A realistic manifest. The platform tags are **dotted** (`macos.debug`, not a
/// bare `macos`) so this exercises the leading-dot-segment match rather than
/// only exact tag equality, and `linux` is listed **first** so a bug that
/// returned the first library regardless of platform fails here on macOS/Windows
/// instead of passing by position.
const MANIFEST: &str = r#"
[configuration]
entry_symbol = "demo_library_init"
compatibility_minimum = "4.4"
reloadable = false

[libraries]
linux.debug.x86_64 = "res://addons/demo/libdemo.linux.so"
windows.debug.x86_64 = "res://addons/demo/demo.windows.dll"
macos.debug = "res://addons/demo/libdemo.macos.dylib"
web.debug.wasm32 = "res://addons/demo/demo.web.wasm"
"#;

/// Mirrors `call_gdextensionmanager_static`'s platform choice, so the expected
/// library below is the one the script path actually selects.
fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_arch = "wasm32") {
        "web"
    } else {
        "linux"
    }
}

/// The library `MANIFEST` should resolve to on the host running this test.
fn expected_library() -> &'static str {
    match platform() {
        "macos" => "res://addons/demo/libdemo.macos.dylib",
        "windows" => "res://addons/demo/demo.windows.dll",
        "web" => "res://addons/demo/demo.web.wasm",
        _ => "res://addons/demo/libdemo.linux.so",
    }
}

/// Every clause is its own function so a failure names the broken call rather
/// than "the script errored".
const GD: &str = r#"
extends Node

func loaded_list():
    return GDExtensionManager.get_loaded_extensions()

func load_missing():
    return GDExtensionManager.load_extension("res://nope/missing.gdextension")

func load_registered():
    return GDExtensionManager.load_extension("res://addons/demo/demo.gdextension")

func is_loaded():
    return GDExtensionManager.is_extension_loaded("res://addons/demo/demo.gdextension")

func is_loaded_unknown():
    return GDExtensionManager.is_extension_loaded("res://nope/missing.gdextension")

func library_of():
    return GDExtensionManager.get_extension("res://addons/demo/demo.gdextension")

func library_of_unknown():
    return GDExtensionManager.get_extension("res://nope/missing.gdextension")

func reload():
    return GDExtensionManager.reload_extension("res://addons/demo/demo.gdextension")

func reload_unknown():
    return GDExtensionManager.reload_extension("res://nope/missing.gdextension")

func unload():
    return GDExtensionManager.unload_extension("res://addons/demo/demo.gdextension")

func singleton_is_known():
    return Engine.has_singleton("GDExtensionManager")
"#;

fn scripted() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let n = tree.add_child(root, Node::new("Ext", "Node")).unwrap();
    tree.attach_script(
        n,
        Box::new(GDScriptNodeInstance::from_source(GD, n).expect("script parses")),
    );
    (tree, n)
}

/// Panics if the call throws — which is exactly the pre-fix behaviour, since
/// `GDExtensionManager` did not resolve as an identifier at all.
fn call(tree: &mut SceneTree, node: NodeId, method: &str) -> Variant {
    tree.call_script_method(node, method, &[])
        .unwrap_or_else(|| panic!("{method} must be callable and must not throw"))
}

/// **The acceptance test.**
#[test]
fn gdextension_manager_singleton() {
    host::reset_host();
    let (mut tree, n) = scripted();

    // -- the singleton exists at all ----------------------------------------
    assert_eq!(
        call(&mut tree, n, "singleton_is_known"),
        Variant::Bool(true),
        "Engine.has_singleton(\"GDExtensionManager\") must be true"
    );

    // -- clean boot: an EMPTY array, not Nil ---------------------------------
    // Nil here is the "wired but inert" failure — it means the singleton arm was
    // never reached and something else answered.
    assert_eq!(
        call(&mut tree, n, "loaded_list"),
        Variant::PackedStringArray(Vec::new()),
        "a clean registry must be an empty PackedStringArray, never Nil"
    );
    assert_eq!(
        call(&mut tree, n, "is_loaded_unknown"),
        Variant::Bool(false),
        "nothing is loaded on a clean boot"
    );
    assert_eq!(
        call(&mut tree, n, "library_of_unknown"),
        Variant::Nil,
        "get_extension() on an unregistered path has no object to return"
    );

    // -- a manifest that cannot be read is FAILED = 1 ------------------------
    // This is the clause whose number the bead gets wrong. Asserting == 1 and
    // != 2 together means neither reading of the bead can pass by accident.
    let missing = call(&mut tree, n, "load_missing");
    assert_eq!(
        missing,
        Variant::Int(1),
        "an unreadable manifest is LoadStatus.FAILED, which Godot numbers 1 \
         (gdextension_manager.h:54-58) — the bead text's \"(2)\" is ALREADY_LOADED"
    );
    assert_ne!(
        missing,
        Variant::Int(2),
        "2 is ALREADY_LOADED — a failed load must not report it"
    );
    assert!(
        !host::is_extension_loaded("res://nope/missing.gdextension"),
        "a failed load must not leave anything in the registry"
    );

    // -- seed the registry, then introspect it from the script ---------------
    assert!(
        host::register_manifest(MANIFEST, platform(), MANIFEST_PATH),
        "the manifest names a library for {} and an entry_symbol, so it registers",
        platform()
    );

    assert_eq!(
        call(&mut tree, n, "is_loaded"),
        Variant::Bool(true),
        "a registered manifest must read back as loaded"
    );
    assert_eq!(
        call(&mut tree, n, "loaded_list"),
        Variant::PackedStringArray(vec![MANIFEST_PATH.to_string()]),
        "get_loaded_extensions() reports MANIFEST paths, not library paths"
    );
    // Platform selection is real: `linux` is first in the manifest, so this
    // fails on a non-Linux host if the resolver ignores the platform.
    assert_eq!(
        call(&mut tree, n, "library_of"),
        Variant::String(expected_library().to_string()),
        "get_extension() must resolve the library for {}, not the first one listed",
        platform()
    );

    // -- loading it twice is ALREADY_LOADED = 2 ------------------------------
    assert_eq!(
        call(&mut tree, n, "load_registered"),
        Variant::Int(2),
        "re-loading a registered manifest is ALREADY_LOADED (2)"
    );

    // -- reload: NEEDS_RESTART = 4 for known, NOT_LOADED = 3 for unknown -----
    // Nothing is executing in-process, so a reload can never take effect; Godot
    // reports NEEDS_RESTART for a non-reloadable extension and this manifest
    // sets `reloadable = false`.
    assert_eq!(
        call(&mut tree, n, "reload"),
        Variant::Int(4),
        "reloading a recognised extension is NEEDS_RESTART (4)"
    );
    assert_eq!(
        call(&mut tree, n, "reload_unknown"),
        Variant::Int(3),
        "reloading something never loaded is NOT_LOADED (3)"
    );

    // -- unload is OK = 0, and is observable; unloading twice is NOT_LOADED --
    assert_eq!(
        call(&mut tree, n, "unload"),
        Variant::Int(0),
        "unloading a registered extension is OK (0)"
    );
    assert_eq!(
        call(&mut tree, n, "is_loaded"),
        Variant::Bool(false),
        "unload must actually remove it — otherwise OK was a lie"
    );
    assert_eq!(
        call(&mut tree, n, "loaded_list"),
        Variant::PackedStringArray(Vec::new()),
        "the registry is empty again after unloading the only extension"
    );
    assert_eq!(
        call(&mut tree, n, "unload"),
        Variant::Int(3),
        "unloading twice is NOT_LOADED (3)"
    );
}

/// The manifest parser, independent of the script surface: an unknown platform
/// and a manifest with no `entry_symbol` must both refuse to register rather
/// than recording a half-built entry.
///
/// Touches no global state, so it is safe alongside the acceptance test under a
/// thread-per-test runner.
#[test]
fn manifest_without_a_usable_library_does_not_register() {
    let cfg = host::parse_gdextension(MANIFEST);
    assert_eq!(cfg.entry_symbol, "demo_library_init");
    assert!(!cfg.reloadable, "the manifest says reloadable = false");
    assert_eq!(cfg.libraries.len(), 4, "four platform tags");
    assert_eq!(
        cfg.library_for_platform("macos"),
        Some("res://addons/demo/libdemo.macos.dylib"),
        "a bare `macos` must match the dotted `macos.debug` tag"
    );
    assert_eq!(
        cfg.library_for_platform("android"),
        None,
        "a platform the manifest does not build for has no library"
    );

    // No `[libraries]` at all, and no entry_symbol: both are refusals, so
    // nothing reaches the registry and the paths below stay unknown.
    assert!(
        !host::register_manifest(
            "[configuration]\nentry_symbol = \"x\"\n",
            "macos",
            "res://a"
        ),
        "a manifest with no libraries section must not register"
    );
    assert!(
        !host::register_manifest(MANIFEST, "android", "res://b"),
        "a manifest with no library for the target platform must not register"
    );
    assert!(
        !host::register_manifest(
            "[libraries]\nmacos = \"res://x.dylib\"\n",
            "macos",
            "res://c"
        ),
        "a manifest with no entry_symbol must not register — there would be \
         nothing to call once loading is implemented"
    );
    for path in ["res://a", "res://b", "res://c"] {
        assert!(
            !host::is_extension_loaded(path),
            "{path} was refused, so it must not be in the registry"
        );
    }
}
