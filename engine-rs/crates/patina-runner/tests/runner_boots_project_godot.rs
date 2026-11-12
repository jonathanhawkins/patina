//! Acceptance test for pat-nzgdu.17: booting patina-runner from `project.godot`.
//!
//! Godot boots a project from `project.godot` (main scene, autoloads, input map,
//! ProjectSettings, res:// root). The runner previously ignored `project.godot`
//! and took a raw `.tscn`. This drives the project boot path against a fixtures
//! project dir and asserts the declared main scene boots, the autoload node
//! exists in the tree, `res://` FileAccess resolves under the project root, and
//! `ProjectSettings` reflects a `project.godot` value.

use std::path::PathBuf;

use patina_runner::project_boot::boot_project;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/project_boot"
    ))
}

#[test]
fn runner_boots_project_godot() {
    let project_dir = fixtures_dir();
    let booted = boot_project(&project_dir).expect("boot from project.godot");

    // (a) The declared main scene (run/main_scene = res://scenes/main.tscn,
    //     root node "Main") is instanced into the tree.
    let main_id = booted.main_scene_root.expect("main scene instanced");
    assert_eq!(
        booted.tree.get_node(main_id).map(|n| n.name()),
        Some("Main"),
        "main scene root should be the declared scene's root node",
    );

    // (b) The [autoload] singleton exists in the tree, reachable as /root/<Name>.
    assert_eq!(booted.autoload_roots.len(), 1, "one autoload declared");
    let (autoload_name, autoload_id) = &booted.autoload_roots[0];
    assert_eq!(autoload_name, "GlobalAutoload");
    let autoload_node = booted
        .tree
        .get_node(*autoload_id)
        .expect("autoload node exists in the tree");
    assert_eq!(
        autoload_node.name(),
        "GlobalAutoload",
        "autoload node is renamed to the autoload's name",
    );

    // (c) res:// FileAccess resolves under the project root (set_path_roots ran).
    let resolved = gdcore::resolve_path("res://scenes/main.tscn")
        .expect("res:// should resolve after boot set the project root");
    assert!(
        resolved.starts_with(&project_dir),
        "res:// path {} should resolve under the project root {}",
        resolved.display(),
        project_dir.display(),
    );
    assert!(
        resolved.exists(),
        "resolved res:// path should exist on disk"
    );

    // (d) ProjectSettings reflects a project.godot value (physics tick rate = 72).
    assert_eq!(
        booted
            .settings
            .get("physics/common/physics_ticks_per_second")
            .and_then(|v| v.as_int()),
        Some(72),
        "ProjectSettings should reflect the physics tick rate from project.godot",
    );

    // (e) The [input] action was installed into the InputMap.
    assert!(
        booted.input_map.actions().any(|a| a == "test_jump"),
        "the declared input action should be registered in the InputMap",
    );
}

/// A directory without a `project.godot` fails to boot with a clear error rather
/// than panicking or silently succeeding.
#[test]
fn boot_project_missing_project_godot_errors() {
    let missing = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
    let result = boot_project(&missing);
    assert!(
        result.is_err(),
        "booting a dir without project.godot should error",
    );
}
