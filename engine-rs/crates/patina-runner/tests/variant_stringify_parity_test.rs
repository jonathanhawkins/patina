//! pat-mgixk.103 — `str()`, `print()` and `Label.text` stringify a Variant the
//! way Godot's `Variant::stringify` does.
//!
//! # What was broken
//!
//! `display_variant` -> `format!("{value}")` -> `impl Display for Variant` used
//! Rust's `{}` for every float, so a running game saw:
//!
//! | expression | Patina | Godot |
//! |---|---|---|
//! | `str(1.0)` | `1` | `1.0` |
//! | `str(0.1 + 0.2)` | `0.30000000000000004` | `0.3` |
//! | `str(Vector2(1, 2))` | `(1, 2)` | `(1.0, 2.0)` |
//! | `str(&"idle")` | `&idle` | `idle` |
//! | `str(NodePath("A/B"))` | `NodePath("A/B")` | `A/B` |
//!
//! Godot routes FLOAT through `String::num_real(v, true)` and every math type's
//! `operator String` through `num_real(component, true)`; STRING_NAME and
//! NODE_PATH stringify to their bare text.
//!
//! ⚠️ `var_to_str` is deliberately NOT affected — it keeps the decorated
//! `&"name"` / `NodePath("..")` / `Vector2(..)` spellings, and it has its own
//! formatter (`interpreter.rs`'s `variant_to_gd_string`), so this change cannot
//! reach it. Scene saving is likewise insulated: `scene_saver` formats values
//! through its own `format_variant_value`/`format_float`.
//!
//! # The entry point
//!
//! A booted project whose `_ready` assigns `str(..)` to a `Label.text` and
//! prints the same string, which is exactly the two paths the Wiring
//! requirement names. The Label text is read back off the live SceneTree and
//! the print is drained through `ScriptInstance::drain_print_output` — the only
//! seam a test has on `print`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `EngineBootstrap` installs process-global `res://` path roots and ClassDB
/// registrations, so two concurrent boots would read each other's roots.
/// Poison-tolerant, bound once per test.
static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Exactly the string the bead's acceptance names.
///
/// Each piece pins a different arm: an integral float, a float whose binary
/// error Godot hides, an f32 math type, a StringName, a NodePath, and an Array
/// of floats.
const EXPECTED: &str = "1.0 0.3 (1.0, 2.0) idle A/B [1.5, 2.0]";

const MAIN_GD: &str = "\
extends Node2D

func _ready():
    var s = str(1.0, \" \", 0.1 + 0.2, \" \", Vector2(1, 2), \" \", &\"idle\", \" \", NodePath(\"A/B\"), \" \", [1.5, 2.0])
    $Label.text = s
    print(s)
    var l = $Label
    l.set_meta(\"one\", str(1.0))
    l.set_meta(\"sum\", str(0.1 + 0.2))
    l.set_meta(\"vec\", str(Vector2(1, 2)))
    l.set_meta(\"sname\", str(&\"idle\"))
    l.set_meta(\"npath\", str(NodePath(\"A/B\")))
    l.set_meta(\"arr\", str([1.5, 2.0]))
";

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk103_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create project dir");
    dir
}

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .expect("write project.godot");
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1_m\"]\n\n\
         [node name=\"Main\" type=\"Node2D\"]\n\
         script = ExtResource(\"1_m\")\n\n\
         [node name=\"Label\" type=\"Label\" parent=\".\"]\n",
    )
    .expect("write main.tscn");
    std::fs::write(dir.join("main.gd"), MAIN_GD).expect("write main.gd");
}

/// **The acceptance test.**
#[test]
fn variant_stringify_parity_test() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("main");
    write_project(&dir);

    let mut boot = EngineBootstrap::new(
        BootConfig::with_scene(dir.join("main.tscn")).project_dir(dir.to_path_buf()),
    );
    boot.run_all().expect("the runner must boot the scene");

    let label = {
        let tree = boot
            .main_loop()
            .expect("the Running phase builds a loop")
            .tree();
        let root = tree.root_id();
        let main = tree
            .find_child(root, "Main", true)
            .expect("the main scene instantiated");
        tree.find_child(main, "Label", true)
            .expect("the scene has a Label")
    };

    // -- per-piece first, so a failure NAMES the arm that diverged ------------
    //
    // 🔑 Without this the only signal would be a diff of two long strings, and
    // every arm below is a separate code path: the combined assertion could not
    // say whether the float, the vector or the NodePath was wrong.
    let piece = |key: &str| -> Variant {
        boot.main_loop()
            .expect("MainLoop")
            .tree()
            .get_node(label)
            .expect("the Label is still in the tree")
            .get_meta(key)
    };
    for (key, want, why) in [
        (
            "one",
            "1.0",
            "an integral float keeps a trailing `.0` (num_real's integral branch); \
             Rust's `{}` gave \"1\"",
        ),
        (
            "sum",
            "0.3",
            "14 significant decimals hide binary representation error; Rust's \
             `{}` gave \"0.30000000000000004\"",
        ),
        (
            "vec",
            "(1.0, 2.0)",
            "a math type runs each component through num_real(x, true), so whole \
             components carry `.0`",
        ),
        (
            "sname",
            "idle",
            "stringify prints a StringName BARE; the `&`-decorated form belongs \
             to var_to_str",
        ),
        (
            "npath",
            "A/B",
            "stringify prints a NodePath BARE, not `NodePath(\"..\")`",
        ),
        (
            "arr",
            "[1.5, 2.0]",
            "an Array stringifies element-wise, so 2.0 keeps its `.0` while 1.5 \
             loses its trailing zeros",
        ),
    ] {
        assert_eq!(
            piece(key),
            Variant::String(want.to_string()),
            "str() of the `{key}` case must be `{want}`: {why}"
        );
    }

    // -- the whole string, through Label.text --------------------------------
    assert_eq!(
        boot.main_loop()
            .expect("MainLoop")
            .tree()
            .get_node(label)
            .expect("the Label is still in the tree")
            .get_property("text"),
        Variant::String(EXPECTED.to_string()),
        "`Label.text = str(..)` must hold Godot's exact stringification"
    );

    // -- and the same string through print() ---------------------------------
    //
    // Both paths go through `display_variant`, so this is a wiring check rather
    // than a second formatting check: it proves the Label assertion above is not
    // passing through some Label-only conversion.
    let prints: Vec<String> = {
        let main_loop = boot.main_loop_mut().expect("MainLoop");
        let tree = main_loop.tree_mut();
        let mut lines = Vec::new();
        for node_id in tree.all_nodes_in_tree_order() {
            if let Some(script) = tree.get_script_mut(node_id) {
                lines.extend(script.drain_print_output());
            }
        }
        lines
    };
    assert!(
        prints.iter().any(|line| line.trim() == EXPECTED),
        "print() must emit the same string; got {prints:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
