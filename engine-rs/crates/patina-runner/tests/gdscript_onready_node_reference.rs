//! pat-mgixk.20 acceptance: `@onready var sprite = $Sprite2D` resolves to the
//! child node, through the runner's real boot.
//!
//! # What was wrong
//!
//! Godot evaluates `@onready` member initializers from
//! `GDScriptInstance::notification`'s `NOTIFICATION_READY` handler
//! (`implicit_ready`), with the node already inside the tree and immediately
//! before the user's `_ready`. That placement is the whole point of the
//! annotation: `$Child` only means anything once the node is in a tree.
//!
//! `SceneTree::process_script_ready` called `script.resolve_onready()` on a
//! borrow out of `self.scripts` with **no `SceneTreeAccessor` installed**, and
//! threw the `Result` away (`let _ =`). `$Sprite2D` evaluates through
//! `Expr::GetNode`, which without scene access returns
//! `Err("get_node() requires scene access")` — so the canonical form resolved
//! to `Nil`, silently, and `resolve_onready_vars`' `?` meant that first failure
//! also abandoned every LATER `@onready` var in the script.
//!
//! Nothing caught it: the V1 gate's `@onready` coverage uses only constant
//! initializers (`@onready var health = 100`), and the repo's only
//! `@onready var x = $...` sources were parse-only and syntax-highlight tests.
//! A textbook false green.
//!
//! # What this drives
//!
//! ```text
//! .gd + .tscn on disk -> EngineBootstrap (the runner's own boot)
//!   -> BootPhase::Scripts   (attaches GDScriptNodeInstance)
//!     -> BootPhase::Lifecycle -> LifecycleManager::enter_tree
//!       -> lifecycle.rs:66 process_script_ready  <- the line under test
//!         -> resolve_onready (now inside an access window) -> _ready
//! ```
//!
//! The observable is the script's own `print`, drained through
//! `ScriptInstance::drain_print_output` — i.e. what a developer watching the
//! runner's output would see, not an internal property read.

use gdscript_interop::bindings::ScriptInstance;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `boot_project`/`EngineBootstrap` install process-global `res://` path roots
/// and ClassDB registrations, so two of these running at once would read each
/// other's roots. A per-process test runner does not need this; a
/// thread-per-test one does. Bound ONCE per test, poison-tolerant.
static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A scratch project directory, removed on drop.
struct Project(std::path::PathBuf);

impl Project {
    fn new(tag: &str, files: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "patina-onready-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the project directory");
        for (name, body) in files {
            std::fs::write(dir.join(name), body).expect("write a project file");
        }
        Self(dir)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Boots `dir`'s `main.tscn` through the runner and returns every marker line
/// its scripts printed.
///
/// Prints are drained from the scripts themselves rather than captured off
/// stdout, because that is the only seam a test has: `print` accumulates in the
/// interpreter's buffer, and `drain_print_output` is what surfaces it.
fn boot_and_collect_prints(dir: &std::path::Path) -> Vec<String> {
    let mut boot = EngineBootstrap::new(
        BootConfig::with_scene(dir.join("main.tscn")).project_dir(dir.to_path_buf()),
    );
    boot.run_all().expect("the runner must boot the scene");

    let main_loop = boot
        .main_loop_mut()
        .expect("the Running phase builds a loop");
    let tree = main_loop.tree_mut();
    let mut lines = Vec::new();
    for node_id in tree.all_nodes_in_tree_order() {
        if let Some(script) = tree.get_script_mut(node_id) {
            lines.extend(script.drain_print_output());
        }
    }
    lines
}

/// **The acceptance test.** `@onready var sprite = $Sprite2D` is the child node
/// by the time `_ready` runs.
#[test]
fn gdscript_onready_node_reference() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let project = Project::new(
        "node-ref",
        &[
            (
                "main.gd",
                "\
extends Node2D

@onready var sprite = $Sprite2D

func _ready():
    print(sprite.name)
",
            ),
            (
                "main.tscn",
                r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="Sprite2D" type="Sprite2D" parent="."]
"#,
            ),
        ],
    );

    let prints = boot_and_collect_prints(project.path());
    assert!(
        prints.iter().any(|line| line.trim() == "Sprite2D"),
        "`@onready var sprite = $Sprite2D` did not resolve to the child node. \
         `_ready` printed {prints:?}. Before this bead `process_script_ready` \
         resolved @onready with NO SceneTreeAccessor installed, so `$Sprite2D` \
         returned Err(\"get_node() requires scene access\") and `sprite` stayed \
         Nil — printing nothing useful at all."
    );
}

/// The second half of the acceptance: one unresolvable initializer must not
/// abandon the ones after it.
///
/// `resolve_onready_vars` used to `eval_expr(expr)?` inside the loop, so the
/// FIRST failure returned early and every later `@onready` var kept its seeded
/// `Nil`. A single bad path therefore emptied the rest of the script's deferred
/// state — and the caller discarded the error, so nothing said so.
#[test]
fn an_onready_var_after_a_failing_one_still_resolves() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let project = Project::new(
        "after-failure",
        &[
            (
                "main.gd",
                "\
extends Node2D

@onready var missing = $NoSuchChild
@onready var sprite = $Sprite2D
@onready var plain = 7

func _ready():
    print(sprite.name)
    print(plain)
",
            ),
            (
                "main.tscn",
                r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="Sprite2D" type="Sprite2D" parent="."]
"#,
            ),
        ],
    );

    let prints = boot_and_collect_prints(project.path());
    // One value per `print` call: `print` joins multiple arguments with a
    // SPACE (`line.join(" ")`), so `print("sprite=", sprite.name)` would emit
    // `sprite= Sprite2D` and an assertion on `sprite=Sprite2D` would fail for a
    // reason that has nothing to do with @onready.
    assert!(
        prints.iter().any(|line| line.trim() == "Sprite2D"),
        "the @onready var DECLARED AFTER a failing one must still resolve. \
         Printed: {prints:?}"
    );
    assert!(
        prints.iter().any(|line| line.trim() == "7"),
        "a constant @onready var after a failing one must still resolve. \
         Printed: {prints:?}"
    );
}

/// A script with `@onready` vars and NO `_ready` must still get them resolved.
///
/// ⚠️ This is why the resolution opens its OWN access window instead of riding
/// on `_ready`'s: `SceneTree::call_script_method` returns early for a method the
/// script does not declare, so folding the two together would leave this script
/// with no window ever opened — and `sprite` permanently `Nil`.
#[test]
fn onready_resolves_for_a_script_with_no_ready_function() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let project = Project::new(
        "no-ready",
        &[
            (
                "main.gd",
                "\
extends Node2D

@onready var sprite = $Sprite2D

func report():
    print(sprite.name)
",
            ),
            (
                "main.tscn",
                r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="Sprite2D" type="Sprite2D" parent="."]
"#,
            ),
        ],
    );

    let mut boot = EngineBootstrap::new(
        BootConfig::with_scene(project.path().join("main.tscn"))
            .project_dir(project.path().to_path_buf()),
    );
    boot.run_all().expect("the runner must boot the scene");
    let main_loop = boot
        .main_loop_mut()
        .expect("the Running phase builds a loop");
    let tree = main_loop.tree_mut();

    // `report` is called explicitly, AFTER boot: the point is that resolution
    // already happened during the ready pass even though there was no `_ready`.
    let node = tree
        .get_node_by_path("/root/Main")
        .expect("the scene root is in the tree");
    tree.call_script_method(node, "report", &[]);

    let mut printed = Vec::new();
    for node_id in tree.all_nodes_in_tree_order() {
        if let Some(script) = tree.get_script_mut(node_id) {
            printed.extend(script.drain_print_output());
        }
    }
    assert!(
        printed.iter().any(|line| line.trim() == "Sprite2D"),
        "a script with @onready vars but no `_ready` must still have them \
         resolved during the ready pass. Printed: {printed:?}"
    );
}
