//! pat-e7t1n: FileAccess / DirAccess Godot-compatible IO parity.
//!
//! Covers the Rust API, ClassDB registration, `res://` resolution, and GDScript
//! static/instance dispatch for open + get_as_text.

use std::fs;
use std::path::PathBuf;

use gdcore::{set_path_roots, DirAccess, FileAccess, FileMode};
use gdobject::class_db::{
    class_exists, clear_for_testing, is_parent_class, register_class, register_io_classes,
    ClassRegistration,
};
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn with_temp_project<F: FnOnce(&PathBuf)>(f: F) {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = dir.path().to_path_buf();
    let user = tempfile::tempdir().expect("user tempdir");
    set_path_roots(Some(project.clone()), Some(user.path().to_path_buf()));
    f(&project);
    set_path_roots(None, None);
}

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    let result = interp.run(src).expect("script should not error");
    result.return_value.expect("expected a return value")
}

#[test]
fn classdb_registers_file_and_dir_access() {
    clear_for_testing();
    register_class(ClassRegistration::new("Object"));
    register_io_classes();
    assert!(class_exists("FileAccess"));
    assert!(class_exists("DirAccess"));
    assert!(is_parent_class("FileAccess", "RefCounted"));
    assert!(is_parent_class("DirAccess", "RefCounted"));
}

#[test]
fn write_read_roundtrip_text_and_bytes() {
    with_temp_project(|project| {
        let path = project.join("roundtrip.txt");
        let path_str = path.to_string_lossy();
        {
            let mut f = FileAccess::open(&path_str, FileMode::Write).unwrap();
            f.store_string("hello").unwrap();
            f.store_buffer(&[0, 1, 2]).unwrap();
        }
        let mut f = FileAccess::open(&path_str, FileMode::Read).unwrap();
        let all = f.get_buffer(usize::MAX).unwrap();
        assert_eq!(&all[..5], b"hello");
        assert_eq!(&all[5..], &[0, 1, 2]);
    });
}

#[test]
fn file_exists_true_false() {
    with_temp_project(|project| {
        let path = project.join("x.bin");
        assert!(!FileAccess::file_exists(&path.to_string_lossy()));
        fs::write(&path, b"1").unwrap();
        assert!(FileAccess::file_exists(&path.to_string_lossy()));
    });
}

#[test]
fn dir_access_lists_created_files() {
    with_temp_project(|project| {
        fs::write(project.join("a.txt"), b"a").unwrap();
        fs::write(project.join("b.txt"), b"b").unwrap();
        let mut dir = DirAccess::open(&project.to_string_lossy()).unwrap();
        dir.list_dir_begin().unwrap();
        let mut names = Vec::new();
        loop {
            let n = dir.get_next();
            if n.is_empty() {
                break;
            }
            names.push(n);
        }
        assert!(names.contains(&"a.txt".to_string()));
        assert!(names.contains(&"b.txt".to_string()));
    });
}

#[test]
fn res_path_resolves_under_project_root() {
    with_temp_project(|project| {
        fs::write(project.join("note.txt"), b"from res").unwrap();
        assert!(FileAccess::file_exists("res://note.txt"));
        let mut f = FileAccess::open("res://note.txt", FileMode::Read).unwrap();
        assert_eq!(f.get_as_text().unwrap(), "from res");
    });
}

#[test]
fn gdscript_file_access_open_and_get_as_text() {
    with_temp_project(|project| {
        fs::write(project.join("script_io.txt"), b"gdscript io works").unwrap();
        let val = run_val(
            r#"
var f = FileAccess.open("res://script_io.txt", FileAccess.READ)
var text = f.get_as_text()
f.close()
return text
"#,
        );
        assert_eq!(val, Variant::String("gdscript io works".into()));
    });
}

#[test]
fn gdscript_file_access_static_get_file_as_string() {
    with_temp_project(|project| {
        fs::write(project.join("s.txt"), b"static path").unwrap();
        let val = run_val(
            r#"
return FileAccess.get_file_as_string("res://s.txt")
"#,
        );
        assert_eq!(val, Variant::String("static path".into()));
    });
}

#[test]
fn gdscript_file_exists() {
    with_temp_project(|project| {
        fs::write(project.join("exists.txt"), b"1").unwrap();
        let val = run_val(
            r#"
return FileAccess.file_exists("res://exists.txt")
"#,
        );
        assert_eq!(val, Variant::Bool(true));
        let missing = run_val(
            r#"
return FileAccess.file_exists("res://nope.txt")
"#,
        );
        assert_eq!(missing, Variant::Bool(false));
    });
}

#[test]
fn gdscript_dir_access_lists_entries() {
    with_temp_project(|project| {
        fs::write(project.join("listed.txt"), b"x").unwrap();
        let val = run_val(
            r#"
var d = DirAccess.open("res://")
d.list_dir_begin()
var found = false
var name = d.get_next()
while name != "":
    if name == "listed.txt":
        found = true
    name = d.get_next()
d.list_dir_end()
return found
"#,
        );
        assert_eq!(val, Variant::Bool(true));
    });
}
