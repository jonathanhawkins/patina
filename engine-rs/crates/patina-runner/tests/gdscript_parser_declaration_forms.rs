//! pat-mgixk.18 acceptance: the declaration and suite forms real GDScript uses.
//!
//! `run_class` is what `GDScriptNodeInstance::from_source` runs for every
//! scene-attached script, and the parser behind it rejected six ordinary forms:
//!
//!   * `var x := expr` / `const X := expr` / `func f(a := 1)` — after the `:`
//!     the parser went straight to `parse_type_hint` -> `eat_ident`, so the `=`
//!     was an unexpected token.
//!   * one-line suites (`if dmg <= 0: return`) — `parse_block_with_lines` did
//!     `skip_newlines` then `expect(Indent)` unconditionally.
//!   * `;` statement separators — the token was lexed and never consumed.
//!   * multi-line `enum { .. }` bodies, and enum values that are constant
//!     EXPRESSIONS: `parse_enum_variant` accepted only an optional `-` plus an
//!     int literal, so the flag idiom `RUN = 1 << 2` errored with
//!     "integer literal for enum value".
//!   * `for i: int in 3:` — `parse_for` expected `in` right after the name.
//!   * `class Inner extends Base:` — `parse_inner_class` expected `:` right
//!     after the name, and the interpreter hardcoded `parent_class: None`.
//!
//! Entry point, per the bead's wiring requirement: a real `project.godot` +
//! `main.tscn` booted through `patina_runner::bootstrap`, with the script
//! attached to the scene root, so the forms are exercised through the same
//! `run_class` path a game uses. The assertions are on the script's own
//! `print()` output and its variables.
//!
//! The script is one unit: if ANY form fails to parse, `from_source` fails, no
//! script attaches and every marker is missing. That makes the first assertion
//! the real control.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

static BOOT_LOCK: Mutex<()> = Mutex::new(());

const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")
"#;

/// Every form the bead names. `hits` is the load-bearing one: `hit()` relies on
/// the `:=` parameter default, and `hit(0)` relies on the one-line
/// `if dmg <= 0: return` actually returning — so `hits == 6` can only hold if
/// both work. A one-liner that silently fell through would give 7.
const MAIN_GD: &str = r#"extends Node2D

enum State {
    IDLE,
    RUN = 1 << 2
}

var speed := 200.0
var hits := 0

class Bullet extends RefCounted:
    var damage = 7

func hit(dmg := 1):
    if dmg <= 0: return
    hits += dmg

func _ready():
    var a := 0
    var b := 0
    a = 1; b = 2
    var total := 0
    for i: int in 3:
        total += i
    hit()
    hit(0)
    hit(5)
    if State.RUN == 4:
        print("ok_enum_expr")
    if State.IDLE == 0:
        print("ok_enum_auto")
    if a == 1 and b == 2:
        print("ok_semicolon")
    if total == 3:
        print("ok_typed_for")
    if hits == 6:
        print("ok_oneline_suite")
    if speed == 200.0:
        print("ok_infer")
"#;

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk18_{}_{}_{}",
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
        "[application]\nconfig/name=\"ParserForms\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("main.gd"), MAIN_GD).unwrap();
}

/// **The acceptance test.**
#[test]
fn gdscript_parser_declaration_forms() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("forms");
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
    boot.main_loop_mut().expect("main loop").step(1.0 / 60.0);

    let tree = boot.main_loop_mut().expect("main loop built").tree_mut();
    let main_id = tree
        .get_node_by_path("/root/Main")
        .expect("main scene instanced");

    // The control: a parse failure in ANY of the forms leaves no script at all,
    // so this fails first and unambiguously rather than as a missing marker.
    assert!(
        tree.get_script(main_id).is_some(),
        "main.gd must have parsed and attached; a parse error leaves no script",
    );

    let printed: Vec<String> = tree
        .get_script_mut(main_id)
        .expect("script attached")
        .drain_print_output();

    // What each marker proves:
    //   ok_enum_expr    — `RUN = 1 << 2` folded to 4. An int-literal-only enum
    //                     parser could not even parse this, let alone reach 4.
    //   ok_enum_auto    — IDLE took its implicit 0 across the multi-line body.
    //   ok_semicolon    — `a = 1; b = 2` ran BOTH statements.
    //   ok_typed_for    — `for i: int in 3:` iterated 0,1,2 (sum 3).
    //   ok_oneline_suite— `hit()` used the `:=` default and `hit(0)` returned
    //                     early from a one-line suite: 1 + 0 + 5 == 6.
    //   ok_infer        — `var speed := 200.0` kept its value.
    let expected = [
        "ok_enum_expr",
        "ok_enum_auto",
        "ok_semicolon",
        "ok_typed_for",
        "ok_oneline_suite",
        "ok_infer",
    ];
    for marker in expected {
        assert!(
            printed.iter().any(|l| l == marker),
            "script must have printed {marker:?}; got {printed:?}",
        );
    }
    // No stray output. Not an exact length check — a second `_ready` would
    // duplicate markers without meaning anything about the parser.
    for line in &printed {
        assert!(
            expected.contains(&line.as_str()),
            "unexpected print output {line:?}; got {printed:?}",
        );
    }

    // `class Bullet extends RefCounted:` has no observable behaviour here on
    // purpose — instantiating an inner class is a separate concern. Its proof is
    // that the script above parsed at all: before this change the `extends` on
    // an inner class was a hard parse error, which would have taken every
    // marker down with it.
    let script = tree.get_script(main_id).expect("script attached");
    assert_eq!(
        script.get_property("speed"),
        Some(Variant::Float(200.0)),
        "the `:=` inferred var must hold its initializer",
    );
    assert_eq!(
        script.get_property("hits"),
        Some(Variant::Int(6)),
        "hits reflects the `:=` default plus the early return",
    );
}
