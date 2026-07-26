//! pat-0q2kx.123 — the `OS` singleton's script surface, backed by one
//! implementation in `gdplatform::os`.
//!
//! Godot's `OS` exposes 83 script methods (extension_api.json). Patina's
//! `call_os_static` answered 15 and errored `UndefinedFunction` for the rest, so
//! `delay_msec`, `shell_open`, `get_config_dir`/`get_cache_dir`/`get_temp_dir`,
//! `set_environment`, `get_version`, `get_executable_path` and the keycode
//! helpers were all unreachable from a script. Worse, two things answered
//! `has_feature` — the interpreter's own table and `gdplatform::os::has_feature`
//! — and they had already drifted apart, and `OS.get_user_data_dir()` returned
//! `<data dir>/Patina` for *every* project, so two games on one machine shared a
//! single `user://`.
//!
//! This drives the real boot path: a `project.godot` names the project and
//! declares a custom feature tag, `boot_project` threads both into
//! `gdplatform::os`, and a `.tscn`-attached script observes the results through
//! the ordinary `OS.*` dispatch.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use gdscene::node::NodeId;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;
use patina_runner::project_boot::boot_project;

const PROJECT_NAME: &str = "OsSurfaceFixture";
const CUSTOM_TAG: &str = "patina_os_surface_tag";

/// Every value comes from a `_ready` running inside the booted scene, so an
/// assertion can only pass if the method is reachable through script dispatch.
const MAIN_SCRIPT: &str = r#"extends Node2D

var user_dir = ""
var threads = false
var config_dir = ""
var cache_dir = ""
var temp_dir = ""
var delayed_ms = -1
var enter_name = ""
var env_value = ""
var env_present = false
var env_after_unset = true
var custom_tag = false
var unknown_tag = true
var exe_path = ""

func _ready():
    self.user_dir = OS.get_user_data_dir()
    self.threads = OS.has_feature("threads")
    self.config_dir = OS.get_config_dir()
    self.cache_dir = OS.get_cache_dir()
    self.temp_dir = OS.get_temp_dir()
    var t0 = OS.get_ticks_msec()
    OS.delay_msec(10)
    self.delayed_ms = OS.get_ticks_msec() - t0
    self.enter_name = OS.get_keycode_string(4194309)
    OS.set_environment("PATINA_OS_SURFACE_T", "1")
    self.env_value = OS.get_environment("PATINA_OS_SURFACE_T")
    self.env_present = OS.has_environment("PATINA_OS_SURFACE_T")
    OS.unset_environment("PATINA_OS_SURFACE_T")
    self.env_after_unset = OS.has_environment("PATINA_OS_SURFACE_T")
    self.custom_tag = OS.has_feature("patina_os_surface_tag")
    self.unknown_tag = OS.has_feature("definitely_not_a_feature")
    self.exe_path = OS.get_executable_path()
"#;

const MAIN_SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1"]

[node name="Main" type="Node2D"]
script = ExtResource("1")
"#;

/// `boot_project` mutates process-global state (the `res://`/`user://` path
/// roots and the OS layer's project name), so these tests must not interleave
/// under a runner that shares one process across tests.
static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_boot() -> std::sync::MutexGuard<'static, ()> {
    BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn unique_project_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Per-process + per-call so a concurrently running copy of this binary
    // cannot collide on the path.
    let dir = std::env::temp_dir().join(format!("patina_0q2kx123_os_{}_{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        format!(
            "config_version=5\n\n\
             [application]\n\n\
             config/name=\"{PROJECT_NAME}\"\n\
             config/features=\"{CUSTOM_TAG}\"\n\
             run/main_scene=\"res://main.tscn\"\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_SCENE).unwrap();
    std::fs::write(dir.join("main.gd"), MAIN_SCRIPT).unwrap();
}

/// Attaches `.gd` sources to the nodes carrying `_script_path`, the same step
/// the runner's boot performs between instancing the scene and `_ready`.
fn attach_scripts(tree: &mut SceneTree, project_dir: &Path) {
    let mut pending: Vec<(NodeId, PathBuf)> = Vec::new();
    for node_id in tree.all_nodes_in_tree_order() {
        if tree.get_script(node_id).is_some() {
            continue;
        }
        if let Some(node) = tree.get_node(node_id) {
            if let Variant::String(res_path) = node.get_property("_script_path") {
                let rel = res_path.strip_prefix("res://").unwrap_or(&res_path);
                pending.push((node_id, project_dir.join(rel)));
            }
        }
    }
    for (node_id, path) in pending {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let instance = GDScriptNodeInstance::from_source(&source, node_id)
            .unwrap_or_else(|e| panic!("parse {}: {e:?}", path.display()));
        tree.attach_script(node_id, Box::new(instance));
    }
}

fn prop(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    tree.get_script(node)
        .expect("main node has a script")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

fn prop_str(tree: &SceneTree, node: NodeId, name: &str) -> String {
    match prop(tree, node, name) {
        Variant::String(s) => s,
        other => panic!("{name} should be a String, got {other:?}"),
    }
}

fn prop_int(tree: &SceneTree, node: NodeId, name: &str) -> i64 {
    match prop(tree, node, name) {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        other => panic!("{name} should be an int, got {other:?}"),
    }
}

#[test]
fn os_singleton_surface_wiring() {
    let _boot = lock_boot();
    let dir = unique_project_dir();
    write_project(&dir);

    let booted = boot_project(&dir).expect("boot the fixture project");
    let mut tree = booted.tree;
    let main_root = booted.main_scene_root.expect("main scene instanced");
    attach_scripts(&mut tree, &dir);
    LifecycleManager::enter_tree(&mut tree, main_root);

    // -- user:// is named after the project, not a shared engine directory ----
    assert_eq!(
        gdplatform::os::project_name(),
        PROJECT_NAME,
        "boot must thread application/config/name into the OS layer",
    );
    let user_dir = prop_str(&tree, main_root, "user_dir");
    let expected_suffix = format!("app_userdata/{PROJECT_NAME}");
    assert!(
        user_dir.ends_with(&expected_suffix),
        "OS.get_user_data_dir() should end with '{expected_suffix}', got '{user_dir}'",
    );

    // -- has_feature has exactly one implementation --------------------------
    assert_eq!(
        prop(&tree, main_root, "threads"),
        Variant::Bool(gdplatform::os::has_feature("threads")),
        "OS.has_feature must route through gdplatform::os::has_feature",
    );
    assert_eq!(
        prop(&tree, main_root, "custom_tag"),
        Variant::Bool(true),
        "a project-declared feature tag must answer true",
    );
    assert_eq!(
        prop(&tree, main_root, "unknown_tag"),
        Variant::Bool(false),
        "an undeclared tag must answer false",
    );

    // -- directories are real and distinct -----------------------------------
    let config_dir = prop_str(&tree, main_root, "config_dir");
    let cache_dir = prop_str(&tree, main_root, "cache_dir");
    let temp_dir = prop_str(&tree, main_root, "temp_dir");
    assert!(
        !config_dir.is_empty(),
        "OS.get_config_dir() must not be empty"
    );
    assert!(
        !cache_dir.is_empty(),
        "OS.get_cache_dir() must not be empty"
    );
    assert!(!temp_dir.is_empty(), "OS.get_temp_dir() must not be empty");
    assert_ne!(
        config_dir, cache_dir,
        "config and cache directories must be distinct",
    );

    // -- delay_msec actually blocks ------------------------------------------
    let delayed = prop_int(&tree, main_root, "delayed_ms");
    assert!(
        delayed >= 10,
        "OS.delay_msec(10) must block at least 10ms as measured by \
         OS.get_ticks_msec(), observed {delayed}ms",
    );

    // -- keycode naming ------------------------------------------------------
    assert_eq!(
        prop_str(&tree, main_root, "enter_name"),
        "Enter",
        "OS.get_keycode_string(KEY_ENTER) must name the key",
    );

    // -- environment round-trip ----------------------------------------------
    assert_eq!(prop_str(&tree, main_root, "env_value"), "1");
    assert_eq!(prop(&tree, main_root, "env_present"), Variant::Bool(true));
    assert_eq!(
        prop(&tree, main_root, "env_after_unset"),
        Variant::Bool(false),
        "OS.unset_environment must remove the variable",
    );

    assert!(
        !prop_str(&tree, main_root, "exe_path").is_empty(),
        "OS.get_executable_path() must report the running binary",
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The keycode helpers are inverses, and modifier bits render Godot's
/// `Ctrl+Alt+Shift+Meta+Key` prefix chain (core/os/keyboard.cpp).
#[test]
fn keycode_string_round_trips_and_renders_modifiers() {
    assert_eq!(gdplatform::os::keycode_string(4194309), "Enter");
    assert_eq!(gdplatform::os::find_keycode_from_string("Enter"), 4194309);
    assert_eq!(gdplatform::os::find_keycode_from_string("enter"), 4194309);
    // The last `+`-separated part is what names the key, as Godot's
    // `find_keycode` does.
    assert_eq!(
        gdplatform::os::find_keycode_from_string("Ctrl+Enter"),
        4194309
    );
    assert_eq!(gdplatform::os::find_keycode_from_string("NotAKey"), 0);

    // Shift is 1 << 25; the key name follows the modifier.
    let shift_enter = 4194309 | (1 << 25);
    assert_eq!(gdplatform::os::keycode_string(shift_enter), "Shift+Enter");
}

/// A project with no `application/config/name` still lands under
/// `app_userdata/<name>` — `ProjectSettings::project_name` supplies Godot's
/// "New Project" default — rather than an empty (or engine-shared) `user://`.
#[test]
fn unnamed_project_falls_back_to_default_name() {
    let _boot = lock_boot();
    let dir = unique_project_dir();
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nconfig/description=\"no name here\"\n",
    )
    .unwrap();

    boot_project(&dir).expect("boot a project with no configured name");
    assert!(
        gdplatform::os::user_data_dir().ends_with("app_userdata/New Project"),
        "an unnamed project should still land under app_userdata/, got '{}'",
        gdplatform::os::user_data_dir(),
    );

    let _ = std::fs::remove_dir_all(&dir);
}
