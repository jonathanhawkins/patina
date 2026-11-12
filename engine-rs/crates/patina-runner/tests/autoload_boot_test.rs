//! End-to-end boot tests for project autoload (global singleton) instantiation.
//!
//! Covers pat-nzgdu.3: a project whose `project.godot` declares an `[autoload]`
//! must, on boot, produce a node under `/root/<Name>` with the autoload's
//! script attached and its `_ready` side effects applied — and it must do so
//! through the real [`EngineBootstrap`] phase sequence (the runner boot path),
//! with the main scene instanced alongside it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, BootPhase, EngineBootstrap};

/// Creates a fresh, unique temp directory for a project fixture.
fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_runner_autoload_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Reads a script variable's current value from a node, or `None`.
fn script_var(
    tree: &gdscene::SceneTree,
    node_id: gdscene::node::NodeId,
    var: &str,
) -> Option<Variant> {
    tree.get_script(node_id)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
}

/// The acceptance test: a project declaring `Global="*res://global.gd"` boots
/// (end-to-end, through `EngineBootstrap`) to a `/root/Global` node whose
/// `_ready` mutation is visible, while the main scene is instanced too.
#[test]
fn autoload_instantiated_under_root() {
    let dir = unique_project_dir("global");
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"AutoloadTest\"\n\n[autoload]\nGlobal=\"*res://global.gd\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("global.gd"),
        "extends Node\nvar loaded = false\nfunc _ready():\n    loaded = true\n",
    )
    .unwrap();
    // A minimal main scene so the boot has something to instance alongside the
    // autoload.
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n",
    )
    .unwrap();

    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.clone(),
        headless: true,
        ..BootConfig::default()
    };

    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");

    assert_eq!(
        boot.autoloads_instantiated(),
        1,
        "one autoload instantiated"
    );

    let tree = boot.main_loop().expect("main loop built").tree();

    // /root/Global exists with the GDScript attached and _ready applied.
    let gid = tree
        .get_node_by_path("/root/Global")
        .expect("/root/Global exists after boot");
    assert_eq!(tree.get_node(gid).unwrap().name(), "Global");
    assert_eq!(
        script_var(tree, gid, "loaded"),
        Some(Variant::Bool(true)),
        "autoload _ready ran during boot"
    );

    // The main scene was instanced alongside the autoload.
    assert!(
        tree.get_node_by_path("/root/Main").is_some(),
        "main scene instanced under /root"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A `.tscn` autoload is instanced under `/root` under its autoload name (the
/// scene's own root name is overridden).
#[test]
fn scene_autoload_instantiated_via_boot() {
    let dir = unique_project_dir("scene");
    std::fs::write(
        dir.join("project.godot"),
        "[autoload]\nHud=\"res://hud.tscn\"\n",
    )
    .unwrap();
    // The scene's root node is named "Layer" — it must be renamed to "Hud".
    std::fs::write(
        dir.join("hud.tscn"),
        "[gd_scene format=3]\n\n[node name=\"Layer\" type=\"CanvasLayer\"]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n",
    )
    .unwrap();

    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.clone(),
        headless: true,
        ..BootConfig::default()
    };

    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");

    assert_eq!(boot.autoloads_instantiated(), 1);
    let tree = boot.main_loop().unwrap().tree();
    let hid = tree
        .get_node_by_path("/root/Hud")
        .expect("/root/Hud exists (scene autoload renamed)");
    assert_eq!(tree.get_node(hid).unwrap().class_name(), "CanvasLayer");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A project with no `[autoload]` section boots cleanly with zero autoloads and
/// the Autoloads phase still runs.
#[test]
fn no_autoloads_when_project_declares_none() {
    let dir = unique_project_dir("none");
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"Empty\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n",
    )
    .unwrap();

    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.clone(),
        headless: true,
        ..BootConfig::default()
    };

    let mut boot = EngineBootstrap::new(config);
    // The Autoloads phase runs but instantiates nothing.
    boot.advance_to(BootPhase::Autoloads)
        .expect("reach autoloads");
    assert_eq!(boot.current_phase(), BootPhase::Autoloads);
    boot.run_all().expect("boot succeeds");
    assert_eq!(boot.autoloads_instantiated(), 0);

    let _ = std::fs::remove_dir_all(&dir);
}
