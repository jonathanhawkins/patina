//! pat-mgixk.17 acceptance: the lexer handles the syntax real GDScript uses.
//!
//! `tokenizer.rs` is the only lexer in the engine (`Interpreter::run_class` and
//! `type_checker` both call `tokenize`), and it was missing seven constructs
//! that appear in essentially every real project:
//!
//!   * `!` — fell through to `UnexpectedChar`, so `if !ready:` was a LEX error.
//!   * `&&` / `||` — lexed as two `Amp`s / two `Pipe`s and died in
//!     `parse_primary`'s `&"StringName"` arm.
//!   * `0xFF` / `0b1010` — the decimal scanner consumed only digits/`.`/`_`, so
//!     these became `IntLit(0)` followed by the identifier `xFF`. Silently the
//!     WRONG VALUE rather than an error, which is the worse failure.
//!   * `1e3` / `1.5e-3` — same shape: `IntLit(1)` plus identifier `e3`.
//!   * `"""triple"""` — a bare newline inside a string was `UnterminatedString`.
//!   * `r"raw"` — no raw form at all, so Windows paths needed double escaping.
//!   * multi-line `[...]` / `(...)` — `Newline` was emitted unconditionally
//!     (no depth tracking anywhere in the file), and `parse_primary` never
//!     skips `Newline`, so a multi-line array or argument list failed to parse.
//!
//! None of the 92 in-repo `.gd` fixtures used any of these, which is exactly why
//! the suite stayed green while the lexer could not read ordinary Godot code.
//!
//! Entry point, per the bead's wiring requirement: a real `project.godot` +
//! `main.tscn` on disk, booted through `patina_runner::bootstrap`, with the
//! script attached to the scene root. The assertions are on the script's own
//! `print()` output and on its variables — what the running game produced — not
//! on token streams. A token-level test would not show that the parser,
//! interpreter and runner all accept the result.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `EngineBootstrap` touches process-global state (ClassDB, resource roots), so
/// the runner tests serialize on one lock.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")
"#;

/// Every construct the bead names, in the spelling a real project would use.
///
/// ⚠️ Each `print` is a bare string literal, never `print("x=", value)`.
/// `print` joins its arguments with a space, so a formatted line would make the
/// assertion depend on that formatting rather than on the lexing under test.
/// The guard conditions carry the actual values.
const MAIN_GD: &str = r#"extends Node2D

const MASK = 0xF0 | 0b1010

var flag = false
var big = 1.5e3
var doc = """line one
line two"""
var path = r"C:\temp\new"
var items = [
    1,
    2,
    3
]

func sum3(a, b, c):
    return a + b + c

func _ready():
    var a = true
    var b = true
    var c = false
    if !flag and (a && b || c):
        print("ok_logic")
    if MASK == 250:
        print("ok_mask")
    if big == 1500.0:
        print("ok_exp")
    if items.size() == 3:
        print("ok_array")
    if sum3(
        10,
        20,
        30
    ) == 60:
        print("ok_call")
"#;

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk17_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"LexerSyntax\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("main.gd"), MAIN_GD).unwrap();
}

/// **The acceptance test.**
#[test]
fn gdscript_lexer_real_world_syntax() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("lexer");
    write_project(&dir);

    gdobject::class_db::register_2d_classes();
    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");
    // One frame, so anything deferred past `_ready` still lands.
    boot.main_loop_mut().expect("main loop").step(1.0 / 60.0);

    let tree = boot.main_loop_mut().expect("main loop built").tree_mut();
    let main_id = tree
        .get_node_by_path("/root/Main")
        .expect("main scene instanced");

    // The control. Before this change the script could not LEX, so
    // `GDScriptNodeInstance::from_source` failed and no script attached at all —
    // which is the first thing to fail loudly, rather than a confusing
    // assertion about missing output further down.
    assert!(
        tree.get_script(main_id).is_some(),
        "main.gd must have parsed and attached; a lex error leaves no script",
    );

    // -- what the running script printed ------------------------------------

    let printed: Vec<String> = tree
        .get_script_mut(main_id)
        .expect("script attached")
        .drain_print_output();

    // What each marker proves, in order:
    //   ok_logic — `if !flag and (a && b || c)`: `!`, `&&` and `||` together.
    //   ok_mask  — `const MASK = 0xF0 | 0b1010` == 240 | 10 == 250. A hex or
    //              binary literal mis-read as `IntLit(0)` can never reach 250,
    //              which is why the guard compares the VALUE and not just that
    //              it lexed.
    //   ok_exp   — `1.5e3 == 1500.0`.
    //   ok_array — a three-element array written across four lines.
    //   ok_call  — a call whose arguments span four lines.
    let expected = ["ok_logic", "ok_mask", "ok_exp", "ok_array", "ok_call"];
    for marker in expected {
        assert!(
            printed.iter().any(|l| l == marker),
            "script must have printed {marker:?}; got {printed:?}",
        );
    }
    // No stray output — every line has to be one of the five. Deliberately NOT
    // an exact length check: `_ready` running twice would duplicate the markers
    // without saying anything about the lexer, whereas an unrecognised line
    // means a guard printed something unexpected.
    for line in &printed {
        assert!(
            expected.contains(&line.as_str()),
            "unexpected print output {line:?}; got {printed:?}",
        );
    }

    // -- and the two string forms, read back off the script -----------------

    let script = tree.get_script(main_id).expect("script attached");
    assert_eq!(
        script.get_property("doc"),
        Some(Variant::String("line one\nline two".to_string())),
        "a triple-quoted string must keep its embedded newline",
    );
    // A raw string performs NO escape processing, so each backslash survives
    // literally. Asserting the exact value is what distinguishes a working raw
    // form from one that silently ate `\t` as a tab.
    assert_eq!(
        script.get_property("path"),
        Some(Variant::String("C:\\temp\\new".to_string())),
        "r\"C:\\temp\\new\" must keep both backslashes verbatim",
    );
}
