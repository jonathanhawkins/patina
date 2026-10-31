//! pat-mgixk.42 acceptance — `ProjectSettings` answers Godot's registered
//! defaults to a script, not just the keys `project.godot` declares.
//!
//! # What was wrong
//!
//! Godot's `GLOBAL_DEF` registers every default into `ProjectSettings::props`
//! (`core/config/project_settings.cpp`), so
//! `get_setting("physics/2d/default_gravity")` answers `980.0` in a project that
//! never overrides it — and Godot never writes a key it has not changed, so a
//! normal `project.godot` is silent about it. That is the classic
//! `CharacterBody2D` idiom.
//!
//! `project_boot` seeded the script-visible store from `ProjectSettings::iter()`,
//! which yields **only declared keys**, so every default came back `Nil` and
//! `has_setting` came back `false`.
//!
//! # The path driven
//!
//! ```text
//! project.godot + main.tscn + main.gd
//!   -> project_boot::boot_project   (what `main.rs` calls, and the only caller
//!      of `seed_project_settings`)
//!     -> seed_project_settings_defaults + seed_project_settings
//!       -> MainLoop::step -> _ready -> ProjectSettings.get_setting / has_setting
//! ```
//!
//! Assertions read what the SCRIPT captured, so they prove the value reached
//! GDScript rather than merely landing in a Rust table.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use gdscene::scripting::GDScriptNodeInstance;
use gdscene::MainLoop;
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;
use patina_runner::project_boot::boot_project;

/// `boot_project` mutates process-global state (path roots, the `res://` roots
/// and the settings store), so these tests must not interleave.
static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_boot() -> std::sync::MutexGuard<'static, ()> {
    BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")
"#;

/// Reads the three settings the acceptance names, plus a key that is neither
/// declared nor documented.
///
/// 🔑 That last one is the guard against over-reach: "seed every default" and
/// "seed everything imaginable" both make the first three assertions pass, and
/// only `has_bogus` tells them apart.
const MAIN_GD: &str = "\
var grav = -1.0
var drag = -1
var has_stretch = false
var has_bogus = true
var atlas = -1

func _ready():
    grav = ProjectSettings.get_setting(\"physics/2d/default_gravity\")
    drag = ProjectSettings.get_setting(\"gui/common/drag_threshold\")
    has_stretch = ProjectSettings.has_setting(\"display/window/stretch/mode\")
    has_bogus = ProjectSettings.has_setting(\"not/a/real/setting\")
    atlas = ProjectSettings.get_setting(\"rendering/2d/shadow_atlas/size\")
";

/// A project that declares NOTHING under `physics/` or `gui/` — exactly the
/// shape Godot writes for a project that has not changed those settings.
const PROJECT_GODOT_SILENT: &str = "config_version=5\n\n\
     [application]\n\n\
     config/name=\"SettingsDefaults\"\n\
     run/main_scene=\"res://main.tscn\"\n";

/// The same project, but overriding the gravity the engine would default.
const PROJECT_GODOT_OVERRIDE: &str = "config_version=5\n\n\
     [application]\n\n\
     config/name=\"SettingsOverride\"\n\
     run/main_scene=\"res://main.tscn\"\n\n\
     [physics]\n\n\
     2d/default_gravity=123\n";

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("patina_mgixk42_{tag}_{}_{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("project.godot"), "").unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("main.gd"), MAIN_GD).unwrap();
    dir
}

/// Boots the project the way the runner binary does (`main.rs` calls
/// `project_boot::boot_project`), then puts the instanced main scene in a
/// `MainLoop` and steps it so `_ready` runs.
///
/// ⚠️ Two halves, because no single entry point does both. `boot_project` is
/// what seeds the script-visible settings store — it is the ONLY caller of
/// `seed_project_settings`, which is the wiring this bead fixes — and it
/// instances the main scene, but script attachment lives in
/// `EngineBootstrap`'s phase machine. `EngineBootstrap` is the mirror image: it
/// attaches scripts and runs `_ready`, and never seeds the settings at all
/// (`bootstrap.rs:427` says it "does not go through `boot_project`"). So the
/// settings come from the real boot path and the script is attached here.
fn boot_and_run(dir: &Path) -> MainLoop {
    let booted = boot_project(dir).expect("boot the fixture project");
    let root = booted
        .main_scene_root
        .expect("boot_project instances the declared main scene");
    let mut tree = booted.tree;
    let source = std::fs::read_to_string(dir.join("main.gd")).unwrap();
    tree.attach_script(
        root,
        Box::new(GDScriptNodeInstance::from_source(&source, root).expect("the script parses")),
    );
    let mut ml = MainLoop::new(tree);
    ml.enter_tree_with_scripts(root);
    ml.step(1.0 / 60.0);
    ml
}

fn script_var(tree: &gdscene::SceneTree, node: gdscene::node::NodeId, var: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
        .unwrap_or(Variant::Nil)
}

/// ⚠️ A `Nil` here is the exact pre-fix symptom, not a broken test: before the
/// defaults were seeded, `get_setting` returned `Nil` for every documented key
/// and the script assigned that to its var. Named in the message so the failure
/// reads as "the default never reached the script" rather than as a type bug.
fn as_f64(v: Variant) -> f64 {
    match v {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        Variant::Nil => panic!(
            "got Nil — `ProjectSettings.get_setting` answered nothing, so the \
             engine default never reached the script"
        ),
        other => panic!("expected a number, got {other:?}"),
    }
}

fn as_i64(v: Variant) -> i64 {
    match v {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        Variant::Nil => panic!(
            "got Nil — `ProjectSettings.get_setting` answered nothing, so the \
             engine default never reached the script"
        ),
        other => panic!("expected a number, got {other:?}"),
    }
}

fn as_bool(v: Variant) -> bool {
    match v {
        Variant::Bool(b) => b,
        other => panic!("expected a bool, got {other:?}"),
    }
}

/// Boots `project_godot` and returns the five values `_ready` captured.
fn captured(tag: &str, project_godot: &str) -> (f64, i64, bool, bool, i64) {
    let dir = unique_project_dir(tag);
    std::fs::write(dir.join("project.godot"), project_godot).unwrap();

    let ml = boot_and_run(&dir);
    let tree = ml.tree();
    let main_id = tree
        .get_node_by_path("/root/Main")
        .expect("the fixture scene has /root/Main");
    (
        as_f64(script_var(tree, main_id, "grav")),
        as_i64(script_var(tree, main_id, "drag")),
        as_bool(script_var(tree, main_id, "has_stretch")),
        as_bool(script_var(tree, main_id, "has_bogus")),
        as_i64(script_var(tree, main_id, "atlas")),
    )
}

#[test]
fn project_settings_script_defaults_test() {
    let _boot = lock_boot();

    // -- a silent project still answers the engine defaults -----------------
    let (grav, drag, has_stretch, has_bogus, atlas) = captured("silent", PROJECT_GODOT_SILENT);

    assert!(
        (grav - 980.0).abs() < 1e-6,
        "ProjectSettings.get_setting(\"physics/2d/default_gravity\") must be \
         Godot's registered 980.0 in a project that never declares it (the \
         CharacterBody2D idiom); got {grav}",
    );
    assert_eq!(
        drag, 10,
        "get_setting(\"gui/common/drag_threshold\") must be the registered 10",
    );
    assert!(
        has_stretch,
        "has_setting(\"display/window/stretch/mode\") must be true for a \
         documented default, as Godot's props table makes it",
    );
    assert!(
        !has_bogus,
        "has_setting must still be FALSE for a key that is neither declared nor \
         documented — otherwise the three assertions above would also pass on a \
         store that claims every key exists",
    );

    // 🔑 This one exercises the WIDENED table, not just the seeding wiring.
    // The three keys above were already in the 61-key hand-curated table, so
    // they would pass on the seeding fix alone; `rendering/2d/shadow_atlas/size`
    // is one of the ~170 keys this bead transcribed from
    // `doc/classes/ProjectSettings.xml`, so it is the half that proves part (2)
    // of the bead rather than part (1).
    assert_eq!(
        atlas, 2048,
        "get_setting(\"rendering/2d/shadow_atlas/size\") must be the documented \
         2048 — a key the default table did not carry before this bead",
    );

    // -- a declared value still wins over the default -----------------------
    let (over_grav, over_drag, _, _, _) = captured("override", PROJECT_GODOT_OVERRIDE);
    assert!(
        (over_grav - 123.0).abs() < 1e-6,
        "a project that DOES declare physics/2d/default_gravity must win over \
         the engine default; got {over_grav}",
    );
    assert_eq!(
        over_drag, 10,
        "and an unrelated default is unaffected by that override",
    );
}

/// `ProjectSettings.save()` must write back only what the project or a script
/// actually set — never the seeded engine defaults.
///
/// 🔑 This is the regression the defaults seeding would otherwise introduce.
/// `save_project_settings_to` walks the whole script-visible store, so once
/// ~230 documented defaults live in it, a plain `save()` would stamp every one
/// of them into the user's `project.godot`. Godot writes only overrides.
#[test]
fn project_settings_save_writes_only_overrides() {
    let _boot = lock_boot();
    let dir = unique_project_dir("save");
    std::fs::write(dir.join("project.godot"), PROJECT_GODOT_SILENT).unwrap();
    let _ml = boot_and_run(&dir);

    let mut interp = Interpreter::new();
    let out = interp
        .run("return ProjectSettings.save()\n")
        .expect("the save script runs")
        .return_value
        .expect("save() returns an Error code");
    assert_eq!(out, Variant::Int(0), "save() must report OK (0)");

    let written = std::fs::read_to_string(dir.join("project.godot"))
        .expect("save() writes back to the booted project.godot");

    assert!(
        written.contains("config/name"),
        "the declared key must survive the round trip:\n{written}",
    );
    for defaulted in [
        "drag_threshold",
        "default_gravity",
        "physics_ticks_per_second",
    ] {
        assert!(
            !written.contains(defaulted),
            "`{defaulted}` is an engine DEFAULT this project never declared, so \
             save() must not write it into project.godot — Godot persists only \
             overrides:\n{written}",
        );
    }
}
