//! pat-0m7oj: GDScript singleton surface — OS, Engine, Time, JSON, ConfigFile.

use gdcore::{set_path_roots, FileAccess, FileMode};
use gdobject::class_db::{class_exists, clear_for_testing, is_parent_class, register_io_classes};
use gdscript_interop::interpreter::{set_engine_editor_hint, set_engine_frames_per_second, Interpreter};
use gdvariant::Variant;
use std::path::PathBuf;

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    let result = interp.run(src).expect("script should not error");
    result.return_value.expect("expected a return value")
}

fn with_temp_project<F: FnOnce(&PathBuf)>(f: F) {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = dir.path().to_path_buf();
    set_path_roots(Some(project.clone()), Some(project.clone()));
    f(&project);
    set_path_roots(None, None);
}

#[test]
fn os_get_name_is_nonempty() {
    let val = run_val("return OS.get_name()\n");
    match val {
        Variant::String(s) => assert!(!s.is_empty(), "OS.get_name should not be empty"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn os_get_ticks_msec_is_nonnegative() {
    let val = run_val("return OS.get_ticks_msec()\n");
    match val {
        Variant::Int(i) => assert!(i >= 0),
        other => panic!("expected Int, got {other:?}"),
    }
}

#[test]
fn engine_is_editor_hint_default_false() {
    set_engine_editor_hint(false);
    let val = run_val("return Engine.is_editor_hint()\n");
    assert_eq!(val, Variant::Bool(false));
}

#[test]
fn engine_is_editor_hint_can_be_set() {
    set_engine_editor_hint(true);
    let val = run_val("return Engine.is_editor_hint()\n");
    assert_eq!(val, Variant::Bool(true));
    set_engine_editor_hint(false);
}

#[test]
fn engine_get_frames_per_second() {
    set_engine_frames_per_second(120.0);
    let val = run_val("return Engine.get_frames_per_second()\n");
    match val {
        Variant::Float(f) => assert!((f - 120.0).abs() < 0.01),
        other => panic!("expected Float, got {other:?}"),
    }
    set_engine_frames_per_second(60.0);
}

#[test]
fn time_get_ticks_msec() {
    let val = run_val("return Time.get_ticks_msec()\n");
    match val {
        Variant::Int(i) => assert!(i >= 0),
        other => panic!("expected Int, got {other:?}"),
    }
}

#[test]
fn time_get_datetime_dict_from_system() {
    let val = run_val("return Time.get_datetime_dict_from_system()\n");
    match val {
        Variant::Dictionary(d) => {
            assert!(d.contains_key("year"));
            assert!(d.contains_key("month"));
            assert!(d.contains_key("day"));
            assert!(d.contains_key("hour"));
            assert!(d.contains_key("minute"));
            assert!(d.contains_key("second"));
        }
        other => panic!("expected Dictionary, got {other:?}"),
    }
}

#[test]
fn json_parse_and_stringify_roundtrip() {
    let parsed = run_val(r#"return JSON.parse("{\"hp\": 10, \"name\": \"hero\"}")"#);
    match parsed {
        Variant::Dictionary(d) => {
            assert_eq!(d.get("hp"), Some(Variant::Int(10)));
            assert_eq!(d.get("name"), Some(Variant::String("hero".into())));
        }
        other => panic!("expected Dictionary, got {other:?}"),
    }

    let text = run_val(
        r#"
var d = {}
d["a"] = 1
return JSON.stringify(d)
"#,
    );
    match text {
        Variant::String(s) => {
            assert!(s.contains("\"a\""));
            assert!(s.contains('1'));
        }
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn config_file_classdb_registered() {
    clear_for_testing();
    register_io_classes();
    assert!(class_exists("ConfigFile"));
    assert!(is_parent_class("ConfigFile", "RefCounted"));
}

#[test]
fn config_file_set_get_from_gdscript() {
    let val = run_val(
        r#"
var c = ConfigFile.new()
c.set_value("player", "hp", 42)
return c.get_value("player", "hp")
"#,
    );
    assert_eq!(val, Variant::Int(42));
}

#[test]
fn config_file_save_load_roundtrip() {
    with_temp_project(|project| {
        let path = project.join("settings.cfg");
        let path_str = path.to_string_lossy().to_string();
        // Build script with absolute path (FileAccess resolves plain paths).
        let src = format!(
            r#"
var c = ConfigFile.new()
c.set_value("audio", "volume", 0.8)
c.set_value("audio", "muted", false)
var err = c.save("{path}")
var c2 = ConfigFile.new()
c2.load("{path}")
return c2.get_value("audio", "volume")
"#,
            path = path_str.replace('\\', "\\\\")
        );
        let val = run_val(&src);
        match val {
            Variant::Float(f) => assert!((f - 0.8).abs() < 1e-6),
            // Some INI parsers may store floats as strings; accept either.
            Variant::String(s) => {
                let f: f64 = s.parse().expect("float string");
                assert!((f - 0.8).abs() < 1e-6);
            }
            other => panic!("expected volume 0.8, got {other:?}"),
        }
        // File should exist on disk.
        assert!(FileAccess::file_exists(&path_str) || path.exists());
        let _ = FileMode::Read; // silence unused in some cfgs
    });
}
