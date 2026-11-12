//! pat-kkqnm — a script that fails to parse is in `--script-output`'s
//! `script_errors`, not only a WARN line on stderr.
//!
//! truck_town's `vehicles/vehicle.gd` and `vehicles/follow_camera.gd` failed to
//! load (pat-rp4bp, pat-0ywbp): the VehicleBody3D and its Camera3D ran with no
//! script, and `scripts/scene-audit.mjs games/truck_town` still reported the
//! scenes as OK, because the only trace was `WARN … failed to parse GDScript`
//! on stderr. The audit reads `script_errors`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn run(dir: &Path, scene: &str, extra: &[&str]) -> Vec<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_patina-runner"))
        .current_dir(dir)
        .arg(scene)
        .args(["--frames", "2", "--script-output"])
        .args(extra)
        .output()
        .expect("failed to execute patina-runner");
    assert!(
        output.status.success(),
        "runner failed:\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let out: serde_json::Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    out["script_errors"]
        .as_array()
        .expect("`script_errors` array")
        .iter()
        .map(|e| e.as_str().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn a_parse_failure_is_a_script_error_naming_the_script_and_line() {
    let dir = std::env::temp_dir().join(format!(
        "patina_kkqnm_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.gd"),
        "extends Node\n\nfunc _ready():\n\tvar x = = 1\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1\"]\n\n[node name=\"Main\" type=\"Node\"]\nscript = ExtResource(\"1\")\n",
    )
    .unwrap();
    let errors = run(&dir, "main.tscn", &[]);
    let hits: Vec<&String> = errors
        .iter()
        .filter(|e| e.contains("script failed to load: res://main.gd: "))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "one load failure for main.gd, got {errors:?}"
    );
    assert!(hits[0].contains("Main"), "names the node: {}", hits[0]);
    assert!(hits[0].contains("line 4"), "names the line: {}", hits[0]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_truck_town_vehicle_script_loads() {
    let game: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/truck_town")
        .canonicalize()
        .expect("games/truck_town fixture");
    for scene in [
        "vehicles/car_base.tscn",
        "vehicles/tow_truck.tscn",
        "vehicles/trailer_truck.tscn",
        "town/town_scene.tscn",
        "car_select/car_select.tscn",
    ] {
        let errors = run(&game, scene, &["--res-root", "."]);
        let failed: Vec<&String> = errors
            .iter()
            .filter(|e| e.contains("script failed to load") || e.contains("member initializer"))
            .collect();
        assert!(
            failed.is_empty(),
            "{scene}: scripts must load, got {failed:?}"
        );
    }
}
