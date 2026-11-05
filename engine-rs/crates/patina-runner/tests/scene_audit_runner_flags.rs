//! pat-iawbc — the runner flags `scripts/scene-audit.mjs` depends on:
//! `--script-output` (what the game's scripts raised), `--class-list` (what
//! ClassDB registers) and `--res-root` (the project as `res://` for a scene in
//! a sub-folder).
//!
//! # The gap `--script-output` closes
//!
//! A scene whose `_ready` calls a method the engine does not have (the
//! platformer_3d demo's `RenderingServer.get_current_rendering_method()`) ran
//! through `patina-runner <scene> --frames 2` with exit 0 and a clean tree
//! dump: the failed call was buffered in the tree's script-error log and never
//! read. `scripts/scene-audit.mjs` loads every scene of an imported game
//! through the runner, so it could not report those errors at all.
//!
//! ⚠️ The missing method is a made-up name on `self`, not a real Godot API
//! the engine lacks today: a test keyed on a real gap would go red the day
//! someone implements it.

use std::path::PathBuf;
use std::process::Command;

fn write_project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "patina_script_output_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\nconfig/name=\"script_output\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.gd"),
        "extends Node\n\nfunc _ready():\n\tprint(\"hello from ready\")\n\tself.patina_no_such_method_xyz()\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1\"]\n\n[node name=\"Main\" type=\"Node\"]\nscript = ExtResource(\"1\")\n",
    )
    .unwrap();
    dir
}

fn run(dir: &PathBuf, extra: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_patina-runner"))
        .current_dir(dir)
        .arg("main.tscn")
        .args(["--frames", "2"])
        .args(extra)
        .output()
        .expect("failed to execute patina-runner");
    assert!(
        output.status.success(),
        "runner failed:\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout is not JSON")
}

#[test]
fn script_output_flag_reports_failed_calls_and_prints() {
    let dir = write_project();
    let out = run(&dir, &["--script-output"]);
    let errors: Vec<String> = out["script_errors"]
        .as_array()
        .expect("`script_errors` must be an array with --script-output")
        .iter()
        .map(|e| e.as_str().unwrap_or_default().to_string())
        .collect();
    assert!(
        errors
            .iter()
            .any(|e| e.contains("patina_no_such_method_xyz")),
        "the failed call must be reported, got {errors:?}"
    );
    let prints = out["script_prints"]
        .as_array()
        .expect("`script_prints` must be an array with --script-output");
    assert!(
        prints
            .iter()
            .any(|p| p.as_str() == Some("hello from ready")),
        "the script's print must be reported, got {prints:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Without the flag the report is unchanged: consumers that assert on the
/// exact key set (goldens, the editor) see no new fields.
#[test]
fn without_the_flag_no_script_output_keys_appear() {
    let dir = write_project();
    let out = run(&dir, &[]);
    assert!(out.get("script_errors").is_none(), "{out}");
    assert!(out.get("script_prints").is_none(), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--class-list` prints the registered ClassDB names as a JSON array, so the
/// scene audit can flag a resource type the engine does not register.
#[test]
fn class_list_flag_prints_registered_classes() {
    let output = Command::new(env!("CARGO_BIN_EXE_patina-runner"))
        .arg("--class-list")
        .output()
        .expect("failed to execute patina-runner");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let list: Vec<String> =
        serde_json::from_slice(&output.stdout).expect("stdout must be a JSON array of names");
    for want in ["Node", "Node3D", "MeshInstance3D", "StandardMaterial3D"] {
        assert!(
            list.iter().any(|c| c == want),
            "{want} missing from {} classes",
            list.len()
        );
    }
    assert!(
        !list.iter().any(|c| c == "PatinaNoSuchClassXyz"),
        "a made-up name must not be listed"
    );
    let mut sorted = list.clone();
    sorted.sort();
    assert_eq!(
        list, sorted,
        "the list is sorted, so the audit is deterministic"
    );
}

/// `--res-root <project>` makes a loose scene in a SUBFOLDER resolve `res://`
/// against the project, so its instanced sub-scenes load. With the scene's own
/// folder as the root (the default, kept for the flat-path fixtures),
/// `res://scenes/child.tscn` was looked up at `scenes/scenes/child.tscn` and
/// the instance silently became an empty `Node` — every nested scene of
/// spark_keeper, platformer and platformer_3d under `scripts/scene-audit.mjs`.
#[test]
fn res_root_flag_instances_res_subscenes_of_a_subfolder_scene() {
    let dir = write_project();
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::write(
        dir.join("scenes/child.tscn"),
        "[gd_scene format=3]\n\n[node name=\"Child\" type=\"Node2D\"]\n\n[node name=\"Inner\" type=\"Sprite2D\" parent=\".\"]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("scenes/holder.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://scenes/child.tscn\" id=\"1\"]\n\n[node name=\"Holder\" type=\"Node2D\"]\n\n[node name=\"Child\" parent=\".\" instance=ExtResource(\"1\")]\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_patina-runner"))
        .current_dir(&dir)
        .args(["scenes/holder.tscn", "--frames", "1", "--res-root", "."])
        .output()
        .expect("failed to execute patina-runner");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let out: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let holder = out["tree"]["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "Holder")
        .expect("Holder under /root")
        .clone();
    let child = &holder["children"][0];
    assert_eq!(child["path"], "/root/Holder/Child");
    assert_eq!(
        child["class"], "Node2D",
        "the instance must be the sub-scene's root, not a placeholder Node: {child}"
    );
    assert_eq!(child["children"][0]["name"], "Inner", "{child}");
    let _ = std::fs::remove_dir_all(&dir);
}
