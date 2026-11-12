//! pat-0q2kx.38 acceptance: `DisplayServer.*` script calls reach an installed
//! platform backend instead of resolving to constants inside the interpreter.
//!
//! Godot ground truth: `platform/web/display_server_web.cpp` — the platform
//! implementation binds clipboard to `navigator.clipboard`, TTS to
//! `SpeechSynthesis`, window size to the canvas, and `has_feature` answers per
//! platform.
//!
//! Patina resolved all of it inside `call_displayserver_static`: window size was
//! a hardcoded 1920x1080, the clipboard a process-local `thread_local` cell, TTS
//! a vector, and `has_feature` always `false`. None of that errored — a game
//! asking for its window size just got a number matching nothing, and every
//! capability query said "unsupported" regardless of the platform.
//!
//! The project below configures a 1152x648 viewport precisely because it differs
//! from the old hardcoded 1920x1080: a test using the default size would pass
//! whether or not the backend seam existed.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdcore::display_backend::clear_display_backend;
use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_0q2kx38_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://probe.gd" id="1_probe"]

[node name="Main" type="Node2D"]
script = ExtResource("1_probe")
"#;

/// Script variables sync onto the node as properties after `_ready`, which is
/// how the test observes what GDScript itself saw.
const PROBE_GD: &str = "\
extends Node2D
var win_size = Vector2(0, 0)
var clip = \"\"
var has_clipboard = false

func _ready():
    win_size = DisplayServer.window_get_size()
    DisplayServer.clipboard_set(\"round-trip\")
    clip = DisplayServer.clipboard_get()
    has_clipboard = DisplayServer.has_feature(5)
";

fn write_project(dir: &Path) {
    write_project_sized(dir, 1152, 648)
}

fn write_project_sized(dir: &Path, w: i64, h: i64) {
    std::fs::write(
        dir.join("project.godot"),
        format!(
            "[application]\nconfig/name=\"DisplayBackend\"\nrun/main_scene=\"res://main.tscn\"\n\
             \n[display]\nwindow/size/viewport_width={w}\nwindow/size/viewport_height={h}\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("probe.gd"), PROBE_GD).unwrap();
}

/// Boots through `EngineBootstrap`, which runs the enter_tree/`_ready`
/// lifecycle. `project_boot::boot_project` instantiates the scene but never
/// drives `_ready`, so a script's variables are never assigned and never sync
/// onto the node — reading them there yields `Nil` regardless of the backend.
fn boot(dir: &Path) -> EngineBootstrap {
    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");
    boot
}

/// **The acceptance test.** Boots a real project and reads back what the script
/// observed.
#[test]
fn displayserver_backend_wiring() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_display_backend();
    let dir = unique_project_dir("wiring");
    write_project(&dir);

    let boot = boot(&dir);
    let tree = boot.main_loop().expect("main loop built").tree();

    // ⚠️ The window size the SCRIPT saw comes from `project.godot`, not from the
    // interpreter's old hardcoded 1920x1080.
    let main = tree.get_node_by_path("/root/Main").expect("Main instanced");
    let node = tree.get_node(main).expect("main node");

    match node.get_property("win_size") {
        Variant::Vector2(v) => {
            assert!(
                (v.x - 1152.0).abs() < 0.5 && (v.y - 648.0).abs() < 0.5,
                "DisplayServer.window_get_size() must report the project's \
                 configured viewport (1152,648), got {v:?} — (1920,1080) means \
                 the call never reached the installed backend"
            );
        }
        other => panic!("script should have read a Vector2 window size, got {other:?}"),
    }

    // The clipboard round-trips through the backend rather than a
    // process-local cell inside the interpreter.
    assert_eq!(
        node.get_property("clip"),
        Variant::String("round-trip".to_string()),
        "clipboard_set followed by clipboard_get must round-trip"
    );

    // has_feature answers the backend's real capability. The old code returned
    // false unconditionally, so a `true` here can only come from the backend.
    assert!(
        matches!(node.get_property("has_clipboard"), Variant::Bool(true)),
        "has_feature(FEATURE_CLIPBOARD) must report the backend's real capability"
    );

    clear_display_backend();
}

/// The acceptance size (1152x648) is ALSO `ProjectSettings`' built-in default,
/// so that test alone cannot distinguish "read from project.godot" from "fell
/// back to the default". This one uses a size that is neither the default nor
/// the old hardcoded 1920x1080, so it can only pass if the value genuinely came
/// out of the file.
#[test]
fn window_size_comes_from_project_godot_not_the_default() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_display_backend();
    let dir = unique_project_dir("nondefault");
    write_project_sized(&dir, 1280, 720);

    let boot = boot(&dir);
    let tree = boot.main_loop().expect("main loop built").tree();
    let main = tree.get_node_by_path("/root/Main").expect("Main instanced");

    match tree
        .get_node(main)
        .expect("main node")
        .get_property("win_size")
    {
        Variant::Vector2(v) => assert!(
            (v.x - 1280.0).abs() < 0.5 && (v.y - 720.0).abs() < 0.5,
            "window size must come from project.godot (1280,720), got {v:?}"
        ),
        other => panic!("expected a Vector2 window size, got {other:?}"),
    }

    clear_display_backend();
}
