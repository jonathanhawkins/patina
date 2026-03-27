//! pat-8gy27: ProjectSettings singleton with godot.project file loading.
//!
//! Validates:
//! 1. Default settings match Godot conventions
//! 2. Get/set/has/remove dynamic property access
//! 3. Convenience accessors (project_name, main_scene, physics_ticks)
//! 4. Section listing and section-filtered queries
//! 5. Parsing Godot INI-style `project.godot` format
//! 6. Value types: bool, int, float, string, Vector2, Vector3, Color
//! 7. Round-trip to/from Godot format
//! 8. File loading and error handling
//! 9. Comments and blank line handling
//! 10. ClassDB registration of ProjectSettings methods

use gdcore::project_settings::{ProjectSettings, SettingsValue};
use std::path::Path;

// ── Defaults ─────────────────────────────────────────────────────────

#[test]
fn defaults_match_godot() {
    let ps = ProjectSettings::new();
    assert_eq!(ps.project_name(), "New Project");
    assert_eq!(ps.main_scene(), "");
    assert_eq!(ps.physics_ticks_per_second(), 60);
    assert_eq!(ps.config_version(), 5);
    assert!(ps.project_path().is_none());
}

#[test]
fn defaults_include_display_settings() {
    let ps = ProjectSettings::new();
    assert_eq!(
        ps.get("display/window/size/viewport_width").unwrap().as_int(),
        Some(1152)
    );
    assert_eq!(
        ps.get("display/window/size/viewport_height").unwrap().as_int(),
        Some(648)
    );
}

#[test]
fn defaults_include_rendering_method() {
    let ps = ProjectSettings::new();
    assert_eq!(
        ps.get("rendering/renderer/rendering_method").unwrap().as_string(),
        Some("forward_plus")
    );
}

#[test]
fn defaults_include_gravity() {
    let ps = ProjectSettings::new();
    assert_eq!(
        ps.get("physics/2d/default_gravity").unwrap().as_float(),
        Some(980.0)
    );
    assert_eq!(
        ps.get("physics/3d/default_gravity").unwrap().as_float(),
        Some(9.8)
    );
}

// ── Get / Set / Has / Remove ─────────────────────────────────────────

#[test]
fn set_and_get_string() {
    let mut ps = ProjectSettings::new();
    ps.set("custom/greeting", SettingsValue::String("hello".into()));
    assert_eq!(ps.get("custom/greeting").unwrap().as_string(), Some("hello"));
}

#[test]
fn set_and_get_int() {
    let mut ps = ProjectSettings::new();
    ps.set("custom/count", SettingsValue::Int(42));
    assert_eq!(ps.get("custom/count").unwrap().as_int(), Some(42));
}

#[test]
fn set_and_get_float() {
    let mut ps = ProjectSettings::new();
    ps.set("custom/ratio", SettingsValue::Float(3.14));
    let val = ps.get("custom/ratio").unwrap().as_float().unwrap();
    assert!((val - 3.14).abs() < f64::EPSILON);
}

#[test]
fn set_and_get_bool() {
    let mut ps = ProjectSettings::new();
    ps.set("custom/flag", SettingsValue::Bool(true));
    assert_eq!(ps.get("custom/flag").unwrap().as_bool(), Some(true));
}

#[test]
fn has_returns_true_for_existing() {
    let ps = ProjectSettings::new();
    assert!(ps.has("application/config/name"));
}

#[test]
fn has_returns_false_for_missing() {
    let ps = ProjectSettings::new();
    assert!(!ps.has("nonexistent/key"));
}

#[test]
fn remove_returns_old_value() {
    let mut ps = ProjectSettings::new();
    ps.set("temp/key", SettingsValue::Int(99));
    let old = ps.remove("temp/key");
    assert_eq!(old.unwrap().as_int(), Some(99));
    assert!(!ps.has("temp/key"));
}

#[test]
fn remove_nonexistent_returns_none() {
    let mut ps = ProjectSettings::new();
    assert!(ps.remove("nonexistent").is_none());
}

#[test]
fn get_or_with_default() {
    let ps = ProjectSettings::new();
    let default = SettingsValue::String("fallback".into());
    let val = ps.get_or("missing/path", &default);
    assert_eq!(val.as_string(), Some("fallback"));
}

#[test]
fn overwrite_setting() {
    let mut ps = ProjectSettings::new();
    ps.set("application/config/name", SettingsValue::String("First".into()));
    ps.set("application/config/name", SettingsValue::String("Second".into()));
    assert_eq!(ps.project_name(), "Second");
}

// ── Convenience accessors ────────────────────────────────────────────

#[test]
fn project_name_accessor() {
    let mut ps = ProjectSettings::new();
    ps.set("application/config/name", SettingsValue::String("My Game".into()));
    assert_eq!(ps.project_name(), "My Game");
}

#[test]
fn main_scene_accessor() {
    let mut ps = ProjectSettings::new();
    ps.set(
        "application/run/main_scene",
        SettingsValue::String("res://main.tscn".into()),
    );
    assert_eq!(ps.main_scene(), "res://main.tscn");
}

#[test]
fn physics_ticks_accessor() {
    let mut ps = ProjectSettings::new();
    ps.set(
        "physics/common/physics_ticks_per_second",
        SettingsValue::Int(120),
    );
    assert_eq!(ps.physics_ticks_per_second(), 120);
}

// ── Sections ─────────────────────────────────────────────────────────

#[test]
fn sections_list_includes_defaults() {
    let ps = ProjectSettings::new();
    let sections = ps.sections();
    assert!(sections.contains(&"application".to_string()));
    assert!(sections.contains(&"physics".to_string()));
    assert!(sections.contains(&"display".to_string()));
    assert!(sections.contains(&"rendering".to_string()));
}

#[test]
fn get_section_filters_by_prefix() {
    let ps = ProjectSettings::new();
    let physics = ps.get_section("physics");
    assert!(physics.len() >= 3);
    for (key, _) in &physics {
        assert!(key.starts_with("physics/"));
    }
}

#[test]
fn custom_section_appears_in_sections() {
    let mut ps = ProjectSettings::new();
    ps.set("autoload/my_singleton", SettingsValue::String("*res://autoload.gd".into()));
    assert!(ps.sections().contains(&"autoload".to_string()));
}

#[test]
fn property_list_and_count() {
    let mut ps = ProjectSettings::new();
    let initial = ps.property_count();
    ps.set("custom/a", SettingsValue::Int(1));
    ps.set("custom/b", SettingsValue::Int(2));
    assert_eq!(ps.property_count(), initial + 2);
    assert!(ps.property_list().contains(&"custom/a"));
    assert!(ps.property_list().contains(&"custom/b"));
}

// ── Godot format parsing ─────────────────────────────────────────────

#[test]
fn parse_basic_godot_format() {
    let content = r#"; Engine configuration file.
config_version=5

[application]
config/name="My Game"
run/main_scene="res://main.tscn"

[physics]
common/physics_ticks_per_second=120
"#;
    let ps = ProjectSettings::from_godot_format(content).unwrap();
    assert_eq!(ps.config_version(), 5);
    assert_eq!(
        ps.get("application/config/name").unwrap().as_string(),
        Some("My Game")
    );
    assert_eq!(
        ps.get("application/run/main_scene").unwrap().as_string(),
        Some("res://main.tscn")
    );
    assert_eq!(
        ps.get("physics/common/physics_ticks_per_second").unwrap().as_int(),
        Some(120)
    );
}

#[test]
fn parse_booleans() {
    let content = "[editor]\nplugin/enabled=true\nother/disabled=false\n";
    let ps = ProjectSettings::from_godot_format(content).unwrap();
    assert_eq!(ps.get("editor/plugin/enabled").unwrap().as_bool(), Some(true));
    assert_eq!(ps.get("editor/other/disabled").unwrap().as_bool(), Some(false));
}

#[test]
fn parse_vector2() {
    let content = "[display]\nwindow/size=Vector2(1920, 1080)\n";
    let ps = ProjectSettings::from_godot_format(content).unwrap();
    match ps.get("display/window/size").unwrap() {
        SettingsValue::Vector2(x, y) => {
            assert!((x - 1920.0).abs() < f64::EPSILON);
            assert!((y - 1080.0).abs() < f64::EPSILON);
        }
        other => panic!("expected Vector2, got {other:?}"),
    }
}

#[test]
fn parse_vector3() {
    let content = "[physics]\ndefault_linear_damp=Vector3(0.1, 0.2, 0.3)\n";
    let ps = ProjectSettings::from_godot_format(content).unwrap();
    match ps.get("physics/default_linear_damp").unwrap() {
        SettingsValue::Vector3(x, y, z) => {
            assert!((x - 0.1).abs() < 0.001);
            assert!((y - 0.2).abs() < 0.001);
            assert!((z - 0.3).abs() < 0.001);
        }
        other => panic!("expected Vector3, got {other:?}"),
    }
}

#[test]
fn parse_color() {
    let content = "[rendering]\nenvironment/default_clear_color=Color(0.3, 0.3, 0.3, 1.0)\n";
    let ps = ProjectSettings::from_godot_format(content).unwrap();
    match ps.get("rendering/environment/default_clear_color").unwrap() {
        SettingsValue::Color(r, _g, _b, a) => {
            assert!((r - 0.3).abs() < 0.001);
            assert!((a - 1.0).abs() < f64::EPSILON);
        }
        other => panic!("expected Color, got {other:?}"),
    }
}

#[test]
fn parse_float_value() {
    let content = "[physics]\n2d/default_gravity=980.5\n";
    let ps = ProjectSettings::from_godot_format(content).unwrap();
    let val = ps.get("physics/2d/default_gravity").unwrap().as_float().unwrap();
    assert!((val - 980.5).abs() < f64::EPSILON);
}

#[test]
fn parse_integer_value() {
    let content = "[display]\nwindow/size/viewport_width=1920\n";
    let ps = ProjectSettings::from_godot_format(content).unwrap();
    assert_eq!(
        ps.get("display/window/size/viewport_width").unwrap().as_int(),
        Some(1920)
    );
}

#[test]
fn skip_comments_and_blanks() {
    let content = "; comment\n# hash comment\n\n[app]\nname=\"Test\"\n";
    let ps = ProjectSettings::from_godot_format(content).unwrap();
    assert_eq!(ps.get("app/name").unwrap().as_string(), Some("Test"));
    assert_eq!(ps.property_count(), 1);
}

#[test]
fn multiple_sections() {
    let content = r#"config_version=5

[application]
config/name="Multi"

[display]
window/size/viewport_width=1280

[rendering]
renderer/rendering_method="mobile"
"#;
    let ps = ProjectSettings::from_godot_format(content).unwrap();
    assert_eq!(ps.get("application/config/name").unwrap().as_string(), Some("Multi"));
    assert_eq!(ps.get("display/window/size/viewport_width").unwrap().as_int(), Some(1280));
    assert_eq!(
        ps.get("rendering/renderer/rendering_method").unwrap().as_string(),
        Some("mobile")
    );
}

// ── Round-trip ───────────────────────────────────────────────────────

#[test]
fn roundtrip_to_godot_format() {
    let mut ps = ProjectSettings::new();
    ps.set("application/config/name", SettingsValue::String("Round Trip".into()));
    ps.set("custom/my_value", SettingsValue::Int(42));

    let output = ps.to_godot_format();
    let ps2 = ProjectSettings::from_godot_format(&output).unwrap();

    assert_eq!(ps2.get("application/config/name").unwrap().as_string(), Some("Round Trip"));
    assert_eq!(ps2.get("custom/my_value").unwrap().as_int(), Some(42));
}

#[test]
fn roundtrip_preserves_config_version() {
    let mut ps = ProjectSettings::new();
    let output = ps.to_godot_format();
    let ps2 = ProjectSettings::from_godot_format(&output).unwrap();
    assert_eq!(ps2.config_version(), ps.config_version());
}

// ── File loading ─────────────────────────────────────────────────────

#[test]
fn load_godot_project_from_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("project.godot");
    std::fs::write(
        &path,
        "; Engine configuration file.\nconfig_version=5\n\n[application]\nconfig/name=\"File Test\"\nrun/main_scene=\"res://levels/main.tscn\"\n",
    )
    .unwrap();

    let ps = ProjectSettings::load_godot_project(&path).unwrap();
    assert_eq!(ps.get("application/config/name").unwrap().as_string(), Some("File Test"));
    assert_eq!(
        ps.get("application/run/main_scene").unwrap().as_string(),
        Some("res://levels/main.tscn")
    );
    assert!(ps.project_path().is_some());
}

#[test]
fn load_missing_file_returns_error() {
    let result = ProjectSettings::load_godot_project(Path::new("/nonexistent/project.godot"));
    assert!(result.is_err());
}

#[test]
fn load_empty_file_returns_empty_settings() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("project.godot");
    std::fs::write(&path, "").unwrap();

    let ps = ProjectSettings::load_godot_project(&path).unwrap();
    assert_eq!(ps.property_count(), 0);
}

// ── SettingsValue type methods ───────────────────────────────────────

#[test]
fn int_promoted_to_float() {
    let v = SettingsValue::Int(60);
    assert_eq!(v.as_float(), Some(60.0));
}

#[test]
fn float_not_accessible_as_int() {
    let v = SettingsValue::Float(3.14);
    assert!(v.as_int().is_none());
}

#[test]
fn string_not_accessible_as_bool() {
    let v = SettingsValue::String("true".into());
    assert!(v.as_bool().is_none());
}

#[test]
fn settings_value_display() {
    assert_eq!(format!("{}", SettingsValue::Bool(true)), "true");
    assert_eq!(format!("{}", SettingsValue::Int(42)), "42");
    assert_eq!(format!("{}", SettingsValue::String("hello".into())), "\"hello\"");
}

// ── ClassDB registration ─────────────────────────────────────────────

#[test]
fn classdb_project_settings_exists() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_exists("ProjectSettings"));
}

#[test]
fn classdb_project_settings_has_methods() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_has_method("ProjectSettings", "get_setting"));
    assert!(gdobject::class_db::class_has_method("ProjectSettings", "set_setting"));
    assert!(gdobject::class_db::class_has_method("ProjectSettings", "has_setting"));
    assert!(gdobject::class_db::class_has_method("ProjectSettings", "save"));
}
