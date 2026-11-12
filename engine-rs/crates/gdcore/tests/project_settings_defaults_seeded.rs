//! pat-0q2kx.126 — a loaded `project.godot` answers with Godot's registered
//! defaults for every key it omits.
//!
//! Godot registers ~300 core defaults via `GLOBAL_DEF`/`GLOBAL_DEF_RST`
//! (`project_settings.cpp:1684-1871`, `main.cpp:2173-3504`,
//! `physics_server_2d.cpp:905-919`, `physics_server_3d.cpp:1141-1154`,
//! `audio_server.cpp:217-218`, `scene_tree.cpp:2064-2184`), and `GLOBAL_GET`
//! always answers with the registered default when the project has not
//! overridden the key. A real `project.godot` therefore declares only the
//! handful of settings it changes.
//!
//! Patina's `ProjectSettings` was a bare key/value bag: `from_godot_format`
//! seeded **zero** defaults, so `get()` returned `None` for every documented
//! key a project omitted and each caller invented its own `unwrap_or` literal.
//!
//! This drives the real `load_godot_project` path with a minimal project file.

use std::path::Path;

use gdcore::project_settings::{ProjectSettings, SettingsValue};

/// A `project.godot` that declares nothing but its name — the shape of a fresh
/// Godot project, and the case that used to return `None` for everything else.
const MINIMAL_PROJECT: &str = r#"; Engine configuration file.
config_version=5

[application]

config/name="Minimal"
"#;

fn load_minimal() -> ProjectSettings {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let path = dir.path().join("project.godot");
    std::fs::write(&path, MINIMAL_PROJECT).expect("write project.godot");
    ProjectSettings::load_godot_project(&path).expect("project loads")
}

fn float_of(ps: &ProjectSettings, key: &str) -> f64 {
    ps.get(key)
        .unwrap_or_else(|| panic!("`{key}` must have a registered default"))
        .as_float()
        .unwrap_or_else(|| panic!("`{key}` should be numeric"))
}

fn int_of(ps: &ProjectSettings, key: &str) -> i64 {
    ps.get(key)
        .unwrap_or_else(|| panic!("`{key}` must have a registered default"))
        .as_int()
        .unwrap_or_else(|| panic!("`{key}` should be an int"))
}

fn string_of(ps: &ProjectSettings, key: &str) -> String {
    ps.get(key)
        .unwrap_or_else(|| panic!("`{key}` must have a registered default"))
        .as_string()
        .unwrap_or_else(|| panic!("`{key}` should be a String"))
        .to_string()
}

fn bool_of(ps: &ProjectSettings, key: &str) -> bool {
    ps.get(key)
        .unwrap_or_else(|| panic!("`{key}` must have a registered default"))
        .as_bool()
        .unwrap_or_else(|| panic!("`{key}` should be a bool"))
}

/// **The acceptance test.** The four keys the bead names, plus enough of the
/// documented table to prove this is a seeded default set and not four
/// special cases — 30 keys across every category the runtime consults.
#[test]
fn project_settings_defaults_seeded() {
    let ps = load_minimal();

    // The four the bead calls out explicitly.
    assert_eq!(float_of(&ps, "physics/2d/default_gravity"), 980.0);
    assert_eq!(string_of(&ps, "display/window/stretch/mode"), "disabled");
    assert_eq!(int_of(&ps, "audio/driver/mix_rate"), 44100);
    assert_eq!(int_of(&ps, "application/run/max_fps"), 0);

    // A key the project DOES declare still wins over the default.
    assert_eq!(
        string_of(&ps, "application/config/name"),
        "Minimal",
        "the project's own value must take precedence over the seeded default",
    );

    // physics/common — main.cpp:2173-2176.
    assert_eq!(int_of(&ps, "physics/common/physics_ticks_per_second"), 60);
    assert_eq!(int_of(&ps, "physics/common/max_physics_steps_per_frame"), 8);
    assert_eq!(float_of(&ps, "physics/common/physics_jitter_fix"), 0.5);

    // physics/2d — physics_server_2d.cpp:905-919.
    assert_eq!(
        ps.get("physics/2d/default_gravity_vector"),
        Some(&SettingsValue::Vector2(0.0, 1.0)),
    );
    assert_eq!(float_of(&ps, "physics/2d/default_linear_damp"), 0.1);
    assert_eq!(float_of(&ps, "physics/2d/default_angular_damp"), 1.0);

    // physics/3d — physics_server_3d.cpp:1141-1154. Note the gravity vector
    // points -Y in 3D and +Y in 2D; seeding them from one table is what keeps
    // that asymmetry correct.
    assert_eq!(float_of(&ps, "physics/3d/default_gravity"), 9.8);
    assert_eq!(
        ps.get("physics/3d/default_gravity_vector"),
        Some(&SettingsValue::Vector3(0.0, -1.0, 0.0)),
    );
    assert_eq!(float_of(&ps, "physics/3d/default_linear_damp"), 0.1);
    assert_eq!(float_of(&ps, "physics/3d/default_angular_damp"), 0.1);

    // display/window.
    assert_eq!(int_of(&ps, "display/window/size/viewport_width"), 1152);
    assert_eq!(int_of(&ps, "display/window/size/viewport_height"), 648);
    assert_eq!(string_of(&ps, "display/window/stretch/aspect"), "keep");
    assert_eq!(float_of(&ps, "display/window/stretch/scale"), 1.0);
    assert!(bool_of(&ps, "display/window/size/resizable"));

    // application/run.
    assert!(!bool_of(&ps, "application/run/low_processor_mode"));
    assert_eq!(string_of(&ps, "application/run/main_scene"), "");

    // rendering — main.cpp:3504.
    assert_eq!(
        string_of(&ps, "rendering/renderer/rendering_method"),
        "forward_plus",
    );
    assert_eq!(
        ps.get("rendering/environment/defaults/default_clear_color"),
        Some(&SettingsValue::Color(0.3, 0.3, 0.3, 1.0)),
    );
    assert_eq!(
        int_of(
            &ps,
            "rendering/textures/canvas_textures/default_texture_filter"
        ),
        1,
    );

    // audio — audio_server.cpp:217-218.
    assert_eq!(int_of(&ps, "audio/driver/output_latency"), 15);

    // gui — scene_tree.cpp:2064-2184.
    assert_eq!(float_of(&ps, "gui/timers/tooltip_delay_sec"), 0.5);
    assert_eq!(int_of(&ps, "gui/common/drag_threshold"), 10);

    // input / navigation / debug.
    assert!(bool_of(
        &ps,
        "input_devices/pointing/emulate_mouse_from_touch"
    ));
    assert_eq!(float_of(&ps, "navigation/3d/default_cell_size"), 0.25);
    assert!(!bool_of(&ps, "debug/settings/stdout/print_fps"));

    // An undocumented key is still absent — the table is a default set, not a
    // catch-all that invents values.
    assert_eq!(ps.get("nonsense/not/a/real/key"), None);
}

/// Seeding must not change what a project *declares*: `to_godot_format` still
/// writes back only the project's own keys, and `property_count` still counts
/// them. Folding the table into the property map would have written ~60
/// unrequested keys into every saved `project.godot`.
#[test]
fn seeded_defaults_do_not_leak_into_the_saved_file() {
    let ps = load_minimal();

    assert_eq!(
        ps.property_count(),
        1,
        "only `application/config/name` was declared by the file",
    );
    assert_eq!(ps.property_list(), vec!["application/config/name"]);

    let saved = ps.to_godot_format();
    assert!(
        saved.contains("config/name=\"Minimal\""),
        "the declared key must round-trip, got:\n{saved}",
    );
    for absent in [
        "default_gravity",
        "mix_rate",
        "tooltip_delay_sec",
        "viewport_width",
    ] {
        assert!(
            !saved.contains(absent),
            "`{absent}` is an engine default the project never set; it must not \
             be written back into project.godot. Output was:\n{saved}",
        );
    }
}

/// `has()` reports registered defaults, matching `ProjectSettings.has_setting`,
/// and `get_default` still shows the engine value behind a project override.
#[test]
fn has_and_get_default_see_through_overrides() {
    let mut ps = load_minimal();

    assert!(
        ps.has("physics/2d/default_gravity"),
        "a registered default counts as an existing setting",
    );
    assert!(!ps.has("nonsense/not/a/real/key"));

    ps.set("physics/2d/default_gravity", SettingsValue::Float(120.0));
    assert_eq!(
        float_of(&ps, "physics/2d/default_gravity"),
        120.0,
        "an override wins",
    );
    assert_eq!(
        ps.get_default("physics/2d/default_gravity"),
        Some(&SettingsValue::Float(980.0)),
        "the engine default remains visible behind the override",
    );

    // Removing the override falls back to the default rather than to nothing.
    let removed = ps.remove("physics/2d/default_gravity");
    assert_eq!(removed, Some(SettingsValue::Float(120.0)));
    assert_eq!(float_of(&ps, "physics/2d/default_gravity"), 980.0);
}

/// `ProjectSettings::new()` gets the same table, so code that never loads a
/// file still reads documented values.
#[test]
fn constructed_settings_carry_the_same_defaults() {
    let ps = ProjectSettings::new();
    assert_eq!(int_of(&ps, "audio/driver/mix_rate"), 44100);
    assert_eq!(float_of(&ps, "physics/3d/default_gravity"), 9.8);
    // Still no file behind it.
    let fresh = ProjectSettings::new();
    assert_eq!(fresh.project_path(), None);
    assert!(ProjectSettings::load_godot_project(Path::new("/nonexistent/project.godot")).is_err());
}
