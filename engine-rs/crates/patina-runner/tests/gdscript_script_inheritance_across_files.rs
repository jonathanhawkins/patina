//! pat-mgixk.21 acceptance — cross-file GDScript inheritance, end to end
//! through the runner.
//!
//! Godot ground truth: `extends "res://base.gd"` makes a script inherit the
//! base script's members, methods and native base class
//! (`modules/gdscript/gdscript.cpp`, `GDScript::_base`); `super(...)` calls the
//! parent's implementation of the method it is written in, and `super.other()`
//! names one explicitly (`gdscript_parser.cpp`).
//!
//! Patina loaded no base script at all. `call_instance_method_impl` saw only
//! `instance.class_def.methods`, `instantiate_class` only the derived class's
//! `instance_vars`, `call_super` hardcoded the parent's `_init`, and
//! `Expr::SuperRef` as a value evaluated to `Nil` — so `super.hit()` dispatched
//! on nothing.
//!
//! Entry point, per the bead's Wiring requirement: a real `project.godot` with
//! four `.gd` files and a `.tscn`, booted by `EngineBootstrap` exactly as the
//! runner boots a game. The assertions are what a developer watching the output
//! would see — the printed lines, in order — plus the script state the scene
//! ends up in.
//!
//! ⚠️ `print` ORDER is the point, not membership. `super()` must run the base's
//! `_ready` BEFORE the child's remaining body, so the sequence must be exactly
//! `["base", "9"]`. Asserting "both lines appear" would pass if the base ran
//! last, or twice.
//!
//! ⚠️ The native-class claim is checked through `load("res://player.gd").new()`
//! rather than off a scene node. A `.tscn` node takes its class from its own
//! `type=` field, so a scene node could never exercise the script chain — it
//! would assert the fixture, not the engine.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `EngineBootstrap` touches process-global engine state. Poison-tolerant so a
/// panic in one test cannot cascade into "lock poisoned" in the next.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk21_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The base: a native `extends`, one member, one method, and a `_ready` that
/// prints so `super()`'s ordering is observable.
const BASE_GD: &str = r#"extends Node2D

var hp = 10

func _ready():
	print("base")

func hit():
	self.hp -= 1
"#;

/// The bead's own derived script: `super()` then the INHERITED `hit()` as a
/// bare call, then print the inherited member.
const PLAYER_GD: &str = r#"extends "res://base.gd"

func _ready():
	super()
	hit()
	print(self.hp)
"#;

/// An override that delegates upward with `super.hit()`.
const OVERRIDE_GD: &str = r#"extends "res://base.gd"

var marker = 0

func hit():
	super.hit()
	self.marker = 1

func _ready():
	hit()
"#;

/// Checks the native class through `.new()`, which is the path
/// `script_base_class` governs.
const PROBE_GD: &str = r#"extends Node

var made_class = "<unset>"

func _ready():
	var n = load("res://player.gd").new()
	self.made_class = n.get_class()
"#;

const MAIN_TSCN: &str = r#"[gd_scene load_steps=4 format=3]

[ext_resource type="Script" path="res://player.gd" id="1_main"]
[ext_resource type="Script" path="res://override.gd" id="2_over"]
[ext_resource type="Script" path="res://probe.gd" id="3_probe"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="Over" type="Node2D" parent="."]
script = ExtResource("2_over")

[node name="Probe" type="Node" parent="."]
script = ExtResource("3_probe")
"#;

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"Inherit\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("base.gd"), BASE_GD).unwrap();
    std::fs::write(dir.join("player.gd"), PLAYER_GD).unwrap();
    std::fs::write(dir.join("override.gd"), OVERRIDE_GD).unwrap();
    std::fs::write(dir.join("probe.gd"), PROBE_GD).unwrap();
}

fn boot(dir: &Path) -> EngineBootstrap {
    // ⚠️ `EngineBootstrap` does NOT configure `res://` — only the full
    // `project_boot` path calls `set_path_roots` (none of bootstrap's four init
    // phases touch it). Without this, `load_base_class` cannot read
    // `res://base.gd`, so the base simply never loads and this test fails with
    // no inheritance rather than with a diagnosis. The same explicit call, for
    // the same reason, is in `gltf_packed_scene_instancing_test.rs:114`. A real
    // game boot goes through `project_boot`, which installs the roots itself.
    gdcore::file_access::set_path_roots(Some(dir.to_path_buf()), None);
    let mut boot = EngineBootstrap::new(
        BootConfig::with_scene(dir.join("main.tscn")).project_dir(dir.to_path_buf()),
    );
    boot.run_all().expect("the runner must boot the scene");
    boot
}

/// Every line any script printed, in tree order.
fn printed(boot: &mut EngineBootstrap) -> Vec<String> {
    let tree = boot
        .main_loop_mut()
        .expect("the Running phase builds a loop")
        .tree_mut();
    let mut lines = Vec::new();
    for node_id in tree.all_nodes_in_tree_order() {
        if let Some(script) = tree.get_script_mut(node_id) {
            lines.extend(script.drain_print_output());
        }
    }
    lines
}

fn script_var(boot: &EngineBootstrap, path: &str, var: &str) -> Variant {
    let tree = boot.main_loop().expect("loop").tree();
    let id = tree
        .get_node_by_path(path)
        .unwrap_or_else(|| panic!("{path} exists in the booted scene"));
    tree.get_script(id)
        .unwrap_or_else(|| panic!("{path} has a script"))
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
        .unwrap_or(Variant::Nil)
}

#[test]
fn gdscript_script_inheritance_across_files() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("across_files");
    write_project(&dir);

    let mut boot = boot(&dir);
    let lines = printed(&mut boot);

    // 🔑 The bead's own claim: `base` then `9`.
    //
    // `base` can only appear if `super()` reached the PARENT's `_ready` rather
    // than its `_init`. `9` can only appear if the bare `hit()` resolved to the
    // inherited method AND the inherited `hp` existed on the instance to be
    // decremented.
    assert_eq!(
        lines,
        vec!["base".to_string(), "9".to_string()],
        "expected the base's `_ready` then the child's print of the inherited, \
         decremented `hp`. Got {lines:?}. An empty list means no script ran at \
         all; `[\"9\"]` means `super()` never reached the parent's `_ready`; \
         `[\"base\", \"10\"]` means the inherited `hit()` did not run; \
         `[\"base\", \"\"]` or a Nil-ish value means `hp` was not inherited."
    );

    // `super.hit()` from an override, on a second node.
    assert_eq!(
        script_var(&boot, "/root/Main/Over", "marker"),
        Variant::Int(1),
        "the override's own body must still run after delegating upward",
    );
    assert_eq!(
        script_var(&boot, "/root/Main/Over", "hp"),
        Variant::Int(9),
        "`super.hit()` must run the PARENT's `hit`, decrementing the inherited \
         `hp` from 10 to 9. A 10 means the delegation silently did nothing — \
         which is what `super` evaluating to Nil used to produce.",
    );

    // The chain resolves to the native ancestor.
    assert_eq!(
        script_var(&boot, "/root/Main/Probe", "made_class"),
        Variant::String("Node2D".into()),
        "player.gd extends base.gd which extends Node2D, so \
         `load(\"res://player.gd\").new()` must create a Node2D; \"Node\" means \
         the `extends` chain was not followed when choosing the node class.",
    );
}
