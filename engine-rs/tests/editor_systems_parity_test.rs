//! Editor systems parity tests.
//!
//! Validates that project settings, editor settings, VCS, export dialog,
//! and variant serialization all behave correctly and cover the expected
//! Godot 4 API surface.

// -- Project settings parity --------------------------------------------------

#[test]
fn project_settings_default_values_match_godot() {
    let ps = gdeditor::settings::ProjectSettings::default();

    // Godot 4 default resolution
    assert_eq!(ps.resolution_w, 1152);
    assert_eq!(ps.resolution_h, 648);

    // Godot 4 default physics
    assert_eq!(ps.physics_fps, 60);
    assert!((ps.default_gravity - 980.0).abs() < 0.1);

    // Godot 4 default stretch
    assert_eq!(ps.stretch_mode, "disabled");
    assert_eq!(ps.stretch_aspect, "keep");

    // Godot 4 default renderer
    assert_eq!(ps.renderer, "forward_plus");
    assert_eq!(ps.anti_aliasing, "disabled");

    // Godot 4 default audio
    assert_eq!(ps.default_bus_layout, "res://default_bus_layout.tres");
    assert!(!ps.enable_audio_input);

    // Vsync on by default
    assert!(ps.vsync);
}

#[test]
fn project_settings_json_roundtrip() {
    let mut ps = gdeditor::settings::ProjectSettings::default();
    ps.project_name = "TestProject".to_string();
    ps.main_scene_path = "res://main.tscn".to_string();
    ps.physics_fps = 120;
    ps.input_map
        .insert("jump".to_string(), vec!["key:Space".to_string()]);

    let json = serde_json::to_string(&ps).unwrap();
    let loaded: gdeditor::settings::ProjectSettings = serde_json::from_str(&json).unwrap();

    assert_eq!(loaded.project_name, "TestProject");
    assert_eq!(loaded.physics_fps, 120);
    assert_eq!(loaded.input_map.get("jump").unwrap()[0], "key:Space");
}

// -- Editor settings parity ---------------------------------------------------

#[test]
fn editor_settings_defaults_match_godot() {
    let es = gdeditor::settings::EditorSettings::default();
    assert_eq!(es.window_size, (1280, 720));
    assert_eq!(es.theme, gdeditor::settings::EditorTheme::Dark);
    assert!(es.auto_save);
    assert!(es.recent_files.is_empty());
}

#[test]
fn editor_settings_recent_files_cap_and_dedup() {
    let mut es = gdeditor::settings::EditorSettings::default();
    for i in 0..25 {
        es.add_recent_file(&format!("res://scene_{i}.tscn"));
    }
    assert_eq!(es.recent_files.len(), 20); // capped at 20

    // Re-adding an existing file moves it to front
    es.add_recent_file("res://scene_10.tscn");
    assert_eq!(es.recent_files[0], "res://scene_10.tscn");
    assert_eq!(es.recent_files.len(), 20); // still capped
}

#[test]
fn editor_settings_json_roundtrip() {
    let dir = std::env::temp_dir().join("patina_editor_settings_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("editor_settings.json");

    let mut es = gdeditor::settings::EditorSettings::default();
    es.window_size = (1920, 1080);
    es.theme = gdeditor::settings::EditorTheme::Light;
    es.auto_save = false;

    es.save(&path).unwrap();
    let loaded = gdeditor::settings::EditorSettings::load(&path).unwrap();

    assert_eq!(loaded.window_size, (1920, 1080));
    assert_eq!(loaded.theme, gdeditor::settings::EditorTheme::Light);
    assert!(!loaded.auto_save);

    std::fs::remove_dir_all(&dir).ok();
}

// -- External editor config ---------------------------------------------------

#[test]
fn external_editor_config_placeholder_expansion() {
    let cfg = gdeditor::settings::ExternalEditorConfig {
        exec_path: "code".to_string(),
        exec_args: vec!["--goto".to_string(), "{file}:{line}:{col}".to_string()],
    };
    assert!(cfg.is_configured());

    let args = cfg.build_args("main.gd", 42, 5);
    assert_eq!(args, vec!["--goto", "main.gd:42:5"]);
}

#[test]
fn external_editor_config_unconfigured() {
    let cfg = gdeditor::settings::ExternalEditorConfig::default();
    assert!(!cfg.is_configured());
}

// -- VCS parity ---------------------------------------------------------------

#[test]
fn vcs_status_model_covers_godot_states() {
    use gdeditor::vcs::{ChangeArea, FileChangeStatus, VcsFileStatus, VcsStatus};

    let status = VcsStatus::default();
    assert!(status.files.is_empty());
    assert!(!status.is_git_repo);

    // Verify all Godot VCS file states are representable
    let entries = vec![
        VcsFileStatus {
            path: "modified.gd".into(),
            status: FileChangeStatus::Modified,
            area: ChangeArea::Unstaged,
        },
        VcsFileStatus {
            path: "added.gd".into(),
            status: FileChangeStatus::Added,
            area: ChangeArea::Staged,
        },
        VcsFileStatus {
            path: "deleted.gd".into(),
            status: FileChangeStatus::Deleted,
            area: ChangeArea::Unstaged,
        },
        VcsFileStatus {
            path: "renamed.gd".into(),
            status: FileChangeStatus::Renamed,
            area: ChangeArea::Staged,
        },
        VcsFileStatus {
            path: "untracked.gd".into(),
            status: FileChangeStatus::Untracked,
            area: ChangeArea::Unstaged,
        },
        VcsFileStatus {
            path: "conflict.gd".into(),
            status: FileChangeStatus::Conflicted,
            area: ChangeArea::Unstaged,
        },
        VcsFileStatus {
            path: "copied.gd".into(),
            status: FileChangeStatus::Copied,
            area: ChangeArea::Staged,
        },
    ];

    assert_eq!(entries.len(), 7); // all 7 Godot VCS states represented
}

#[test]
fn vcs_branch_info_model() {
    use gdeditor::vcs::BranchInfo;

    let branch = BranchInfo {
        name: "main".to_string(),
        ahead: 2,
        behind: 1,
        detached: false,
    };
    assert_eq!(branch.name, "main");
    assert_eq!(branch.ahead, 2);
    assert_eq!(branch.behind, 1);
    assert!(!branch.detached);
}

// -- Export dialog parity -----------------------------------------------------

#[test]
fn export_dialog_platform_coverage() {
    use gdeditor::export_dialog::{ExportDialog, ExportPlatform, ExportPreset};

    let mut dialog = ExportDialog::new();

    // All major Godot export platforms must be representable
    let platforms = [
        ExportPlatform::Windows,
        ExportPlatform::Linux,
        ExportPlatform::MacOS,
        ExportPlatform::Web,
        ExportPlatform::Android,
        ExportPlatform::IOS,
    ];

    for platform in &platforms {
        let preset = ExportPreset::new(format!("{platform:?}"), *platform);
        dialog.add_preset(preset).unwrap();
    }

    assert_eq!(dialog.presets().len(), 6);
}

#[test]
fn export_preset_properties() {
    use gdeditor::export_dialog::{ExportBuildProfile, ExportDialog, ExportPlatform, ExportPreset};

    let mut dialog = ExportDialog::new();
    dialog
        .add_preset(ExportPreset::new("Release Build", ExportPlatform::Linux))
        .unwrap();

    let preset = &dialog.presets()[0];
    assert_eq!(preset.name, "Release Build");
    assert_eq!(preset.platform, ExportPlatform::Linux);
    assert_eq!(preset.build_profile, ExportBuildProfile::Release); // default
}

// -- Variant serialization coverage -------------------------------------------

#[test]
fn variant_transform2d_roundtrip() {
    use gdcore::math::{Transform2D, Vector2};
    use gdvariant::serialize::{from_json, to_json};
    use gdvariant::Variant;

    let t = Transform2D {
        x: Vector2::new(0.866, -0.5),
        y: Vector2::new(0.5, 0.866),
        origin: Vector2::new(100.0, 200.0),
    };
    let v = Variant::Transform2D(t);
    let json = to_json(&v);
    let rt = from_json(&json).expect("Transform2D roundtrip failed");
    assert_eq!(rt, v);
}

#[test]
fn variant_callable_method_roundtrip() {
    use gdvariant::serialize::{from_json, to_json};
    use gdvariant::{CallableRef, Variant};

    let v = Variant::Callable(Box::new(CallableRef::Method {
        target_id: 99,
        method: "on_damage".to_string(),
    }));
    let json = to_json(&v);
    let rt = from_json(&json).expect("Callable roundtrip failed");
    assert_eq!(rt, v);
}

#[test]
fn variant_all_types_serialize() {
    use gdcore::math::*;
    use gdcore::math3d::*;
    use gdvariant::serialize::to_json;
    use gdvariant::Variant;

    // Every variant type must produce valid JSON without panicking
    let variants = vec![
        Variant::Nil,
        Variant::Bool(true),
        Variant::Int(42),
        Variant::Float(3.14),
        Variant::String("test".into()),
        Variant::StringName(gdcore::StringName::new("name")),
        Variant::NodePath(gdcore::NodePath::new("/root/Node")),
        Variant::Vector2(Vector2::new(1.0, 2.0)),
        Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)),
        Variant::Rect2(Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0))),
        Variant::Transform2D(Transform2D {
            x: Vector2::new(1.0, 0.0),
            y: Vector2::new(0.0, 1.0),
            origin: Vector2::ZERO,
        }),
        Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
        Variant::Basis(Basis::IDENTITY),
        Variant::Transform3D(Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::ZERO,
        }),
        Variant::Quaternion(Quaternion::new(0.0, 0.0, 0.0, 1.0)),
        Variant::Aabb(Aabb::new(Vector3::ZERO, Vector3::new(1.0, 1.0, 1.0))),
        Variant::Plane(Plane::new(Vector3::new(0.0, 1.0, 0.0), 0.0)),
        Variant::ObjectId(gdcore::id::ObjectId::from_raw(1)),
        Variant::Array(vec![Variant::Int(1)]),
        Variant::Dictionary(std::collections::HashMap::new()),
    ];

    for v in &variants {
        let json = to_json(v);
        assert!(
            json.is_object(),
            "variant {:?} didn't serialize to object",
            v
        );
    }
    assert_eq!(variants.len(), 20); // 20 variant types covered
}

// -- ClassDB editor class registrations ---------------------------------------

#[test]
fn classdb_editor_settings_methods_registered() {
    gdobject::class_db::register_editor_classes();

    assert!(gdobject::class_db::class_exists("EditorInterface"));
    assert!(gdobject::class_db::class_has_method(
        "EditorInterface",
        "get_editor_settings"
    ));
    assert!(gdobject::class_db::class_has_method(
        "EditorInterface",
        "save_scene"
    ));
}

#[test]
fn classdb_editor_plugin_methods_registered() {
    gdobject::class_db::register_editor_classes();

    assert!(gdobject::class_db::class_exists("EditorPlugin"));
    assert!(gdobject::class_db::class_has_method(
        "EditorPlugin",
        "get_editor_interface"
    ));
    assert!(gdobject::class_db::class_has_method(
        "EditorPlugin",
        "add_custom_type"
    ));
    assert!(gdobject::class_db::class_has_method(
        "EditorPlugin",
        "add_autoload_singleton"
    ));
}
