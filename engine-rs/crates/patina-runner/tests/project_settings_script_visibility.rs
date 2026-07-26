//! pat-0q2kx.135 — a booted project's settings are visible to the scripts
//! running inside it, feature-tag overrides resolve, and `save()` writes a file
//! that reloads.
//!
//! Godot's `ProjectSettings` singleton holds every `project.godot` value and
//! `get_setting_with_override` prefers `setting.<tag>` for an active feature tag
//! (core/config/project_settings.cpp).
//!
//! Patina parsed `project.godot` correctly at boot into `gdcore::ProjectSettings`
//! — which has implemented override resolution all along — but the interpreter's
//! singleton was a disconnected thread-local `HashMap` seeded by NOTHING, with a
//! self-documented TODO. So a script inside a project that plainly declares
//! `physics/2d/default_gravity = 123` read back `Nil`, and
//! `get_setting_with_override` ignored feature tags entirely.
//!
//! ⚠️ Every value here is observed from a GDScript running after `boot_project`,
//! through the ordinary `ProjectSettings.*` dispatch — not by reading the Rust
//! struct, which already worked.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;
use patina_runner::project_boot::boot_project;

/// `boot_project` mutates process-global state (path roots, the OS layer's
/// project name, and the settings store), so these tests must not interleave.
static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_boot() -> std::sync::MutexGuard<'static, ()> {
    BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_0q2kx135_{tag}_{}_{}",
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The fixture the acceptance names: a non-default gravity, a viewport width,
/// and a `.web` override of that width.
const PROJECT_GODOT: &str = "config_version=5\n\n\
     [application]\n\n\
     config/name=\"SettingsVisibility\"\n\n\
     [display]\n\n\
     window/size/viewport_width=111\n\
     window/size/viewport_width.web=222\n\n\
     [physics]\n\n\
     2d/default_gravity=123\n";

/// Runs `src` in a fresh interpreter and returns its `return` value.
fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n--- source ---\n{src}"))
        .return_value
        .expect("script returned a value")
}

#[test]
fn project_settings_script_visibility() {
    let _boot = lock_boot();
    let dir = unique_project_dir("visibility");
    std::fs::write(dir.join("project.godot"), PROJECT_GODOT).unwrap();

    boot_project(&dir).expect("boot the fixture project");

    // -- the project's own values reach a script ---------------------------
    assert_eq!(
        run_val("return ProjectSettings.get_setting(\"physics/2d/default_gravity\")\n"),
        Variant::Int(123),
        "a script must read the gravity the project declares, not Nil"
    );
    assert_eq!(
        run_val("return ProjectSettings.get_setting(\"display/window/size/viewport_width\")\n"),
        Variant::Int(111),
        "a script must read the declared viewport width"
    );
    assert_eq!(
        run_val("return ProjectSettings.has_setting(\"physics/2d/default_gravity\")\n"),
        Variant::Bool(true),
        "has_setting must see the seeded settings"
    );
    assert_eq!(
        run_val("return ProjectSettings.has_setting(\"not/a/real/setting\")\n"),
        Variant::Bool(false),
        "has_setting must still answer false for an undeclared key"
    );
    // Godot returns the caller's default for a missing setting.
    assert_eq!(
        run_val("return ProjectSettings.get_setting(\"nope/missing\", 7)\n"),
        Variant::Int(7),
        "a missing setting returns the caller's default"
    );

    // -- feature-tag override ----------------------------------------------
    // The `.web` override must win only when the `web` tag is active. This test
    // binary is a native build, so `web` is NOT active and the base value wins;
    // activating the tag flips it to 222 through the same call.
    assert_eq!(
        run_val(
            "return ProjectSettings.get_setting_with_override(\"display/window/size/viewport_width\")\n"
        ),
        Variant::Int(111),
        "with no web tag active the base value must win"
    );
    gdscript_interop::interpreter::set_project_feature_tags(["web"]);
    assert_eq!(
        run_val(
            "return ProjectSettings.get_setting_with_override(\"display/window/size/viewport_width\")\n"
        ),
        Variant::Int(222),
        "with the web tag active, viewport_width.web must win"
    );
    // A setting with no override for the active tag still resolves to its base.
    assert_eq!(
        run_val(
            "return ProjectSettings.get_setting_with_override(\"physics/2d/default_gravity\")\n"
        ),
        Variant::Int(123),
        "a setting with no .web variant falls back to its base value"
    );

    // -- save() round-trips through a reloadable file ----------------------
    let saved = run_val(
        "ProjectSettings.set_setting(\"physics/2d/default_gravity\", 456)\n\
         return ProjectSettings.save()\n",
    );
    assert_eq!(saved, Variant::Int(0), "save() must report OK (0)");

    let written = std::fs::read_to_string(dir.join("project.godot"))
        .expect("save() must write back to the booted project.godot");
    assert!(
        written.contains("2d/default_gravity=456"),
        "the saved file must carry the new value:\n{written}"
    );

    // The real requirement: the file RELOADS, not merely that bytes changed.
    let reloaded = gdcore::ProjectSettings::load_godot_project(&dir.join("project.godot"))
        .expect("the saved project.godot must parse again");
    assert_eq!(
        reloaded
            .get("physics/2d/default_gravity")
            .and_then(|v| v.as_int()),
        Some(456),
        "the reloaded file must carry the saved value"
    );
    assert_eq!(
        reloaded
            .get("display/window/size/viewport_width")
            .and_then(|v| v.as_int()),
        Some(111),
        "saving must not drop the settings the script did not touch"
    );
    assert_eq!(
        reloaded
            .get("display/window/size/viewport_width.web")
            .and_then(|v| v.as_int()),
        Some(222),
        "saving must preserve feature-tag override keys"
    );

    gdscript_interop::interpreter::reset_project_settings();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A script's own `set_setting` is visible immediately and does not disturb the
/// seeded values — the store is shared, not two competing maps.
#[test]
fn script_writes_and_seeded_values_share_one_store() {
    let _boot = lock_boot();
    let dir = unique_project_dir("share");
    std::fs::write(dir.join("project.godot"), PROJECT_GODOT).unwrap();
    boot_project(&dir).expect("boot");

    assert_eq!(
        run_val(
            "ProjectSettings.set_setting(\"my/own/key\", \"hello\")\n\
             return ProjectSettings.get_setting(\"my/own/key\")\n"
        ),
        Variant::String("hello".to_string()),
        "a script's own setting round-trips"
    );
    // ...and the project's value is still there alongside it.
    assert_eq!(
        run_val("return ProjectSettings.get_setting(\"physics/2d/default_gravity\")\n"),
        Variant::Int(123),
        "a script write must not clear the seeded project values"
    );

    gdscript_interop::interpreter::reset_project_settings();
    let _ = std::fs::remove_dir_all(&dir);
}

/// With no project booted there is nowhere to save to, so `save()` reports
/// Godot's ERR_FILE_CANT_OPEN rather than silently claiming success.
#[test]
fn save_without_a_booted_project_reports_an_error() {
    let _boot = lock_boot();
    gdscript_interop::interpreter::reset_project_settings();
    assert_eq!(
        run_val("return ProjectSettings.save()\n"),
        Variant::Int(12),
        "save() with no seeded project path must report ERR_FILE_CANT_OPEN (12)"
    );
}
