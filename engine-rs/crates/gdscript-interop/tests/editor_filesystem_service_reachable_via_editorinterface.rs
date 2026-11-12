//! pat-0q2kx.167 — **the acceptance test**: a `@tool` script reaches the editor
//! filesystem service through `EditorInterface` and walks it.
//!
//! The bead's acceptance names `editor_filesystem_service_reachable_via_editorinterface`.
//! That test did not exist; the bead had instead accumulated four green runs
//! against substitute commands (`editor_filesystem_classes_registered`,
//! `gdeditor --lib list_dir`, `editor_paths_singleton_script_reachable`), each
//! covering a different clause and none covering the headline one.
//!
//! Those substitutes were not wrong — they verified real work. But the class
//! registration described a surface no script could reach: `EditorInterface` did
//! not exist as a name in the interpreter at all, so
//! `EditorInterface.get_resource_filesystem()` failed before any of the
//! registration mattered. This test pins the end-to-end path.
//!
//! The project root is pointed at a temp tree via
//! `gdcore::file_access::set_path_roots`, so "matching the real project tree"
//! is asserted against a tree this test builds rather than against whatever
//! happens to be checked out — the assertion is exact instead of approximate.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Serializes the tests in this binary that touch the res:// path roots (each test roots res:// at its own temp project) (pat-hukbz).
///
/// That state is process-global. Nextest gives each test its own process, but
/// plain `cargo test` runs the tests as threads in ONE process, where they
/// overwrote each other's state mid-test. Bind the guard for the whole test.
/// Poison-tolerant, so one failing test does not fail the rest.
#[must_use = "bind it for the whole test: `let _guard = serial();`"]
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Builds a small project tree and points `res://` at it.
///
/// Returns the temp dir; the caller must keep it alive for the test's duration
/// or the directory is removed while the script is walking it.
fn project_fixture() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "patina_efs_{}_{}",
        std::process::id(),
        // Distinct per test within a process; nextest also gives each test its
        // own process, so this only has to break ties within a file.
        std::thread::current()
            .name()
            .unwrap_or("t")
            .replace("::", "_")
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("scenes")).expect("mkdir scenes");
    std::fs::create_dir_all(root.join("art").join("sprites")).expect("mkdir art/sprites");
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("write project.godot");
    std::fs::write(root.join("scenes").join("main.tscn"), b"[gd_scene]\n").expect("write main");
    std::fs::write(
        root.join("art").join("sprites").join("hero.png"),
        b"\x89PNG",
    )
    .expect("write hero");
    gdcore::file_access::set_path_roots(Some(root.clone()), None);
    root
}

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

fn arr(v: &Variant) -> Vec<Variant> {
    match v {
        Variant::Array(items) => items.to_vec(),
        other => panic!("expected an array, got {other:?}"),
    }
}

fn strs(v: &Variant) -> Vec<String> {
    arr(v)
        .iter()
        .map(|e| match e {
            Variant::String(s) => s.clone(),
            other => panic!("expected a string, got {other:?}"),
        })
        .collect()
}

/// **The acceptance.** Reach the service through `EditorInterface`, walk
/// `EditorFileSystemDirectory`, and enumerate files matching the real tree.
#[test]
fn editor_filesystem_service_reachable_via_editorinterface() {
    let _guard = serial();
    let _root = project_fixture();

    // Walk the root: its own path, its subdirectories, and its files.
    let v = run_val(
        "var fs = EditorInterface.get_resource_filesystem()\n\
         var dir = fs.get_filesystem()\n\
         var subs = []\n\
         for i in range(dir.get_subdir_count()):\n\
         \tsubs.append(dir.get_subdir(i).get_name())\n\
         var files = []\n\
         for i in range(dir.get_file_count()):\n\
         \tfiles.append(dir.get_file(i))\n\
         return [dir.get_path(), subs, files]\n",
    );
    let parts = arr(&v);
    assert_eq!(
        parts[0],
        Variant::String("res://".into()),
        "the filesystem root is res://"
    );

    let mut subs = strs(&parts[1]);
    subs.sort();
    assert_eq!(
        subs,
        vec!["art".to_string(), "scenes".to_string()],
        "the walk must enumerate the real subdirectories of the project root"
    );

    let files = strs(&parts[2]);
    assert_eq!(
        files,
        vec!["project.godot".to_string()],
        "and the real files — this is the clause the substitute commands never \
         covered, because it needs the whole EditorInterface -> EditorFileSystem \
         -> EditorFileSystemDirectory chain to exist"
    );
}

/// The walk descends, and `get_file_path` yields usable `res://` paths — a walk
/// that produced bare names would enumerate the tree but be unable to open
/// anything it found.
#[test]
fn walking_into_a_subdirectory_yields_resource_paths() {
    let _guard = serial();
    let _root = project_fixture();

    let v = run_val(
        "var dir = EditorInterface.get_resource_filesystem().get_filesystem()\n\
         var art = null\n\
         for i in range(dir.get_subdir_count()):\n\
         \tif dir.get_subdir(i).get_name() == \"art\":\n\
         \t\tart = dir.get_subdir(i)\n\
         var sprites = art.get_subdir(0)\n\
         return [sprites.get_path(), sprites.get_file_path(0), sprites.get_parent().get_name()]\n",
    );
    let parts = arr(&v);
    assert_eq!(
        parts[0],
        Variant::String("res://art/sprites/".into()),
        "a nested directory reports its full res:// path with a trailing slash"
    );
    assert_eq!(
        parts[1],
        Variant::String("res://art/sprites/hero.png".into()),
        "get_file_path joins the directory path with the file name"
    );
    assert_eq!(
        parts[2],
        Variant::String("art".into()),
        "get_parent climbs back up the tree"
    );
}

/// Godot terminates an upward walk with `null` at the root. Without this a
/// script climbing with `while d.get_parent() != null` never stops.
#[test]
fn the_root_has_no_parent() {
    let _guard = serial();
    let _root = project_fixture();

    let v = run_val(
        "var dir = EditorInterface.get_resource_filesystem().get_filesystem()\n\
         return dir.get_parent() == null\n",
    );
    assert_eq!(v, Variant::Bool(true), "res:// is the top of the walk");
}

/// `EditorPaths.get_cache_dir()` — the acceptance's second clause. Already
/// working before this bead's code changes; asserted here so the acceptance
/// test covers every clause it names rather than most of them.
#[test]
fn editor_paths_cache_dir_resolves() {
    let _guard = serial();
    let v = run_val("return EditorPaths.get_cache_dir()\n");
    match v {
        Variant::String(s) => assert!(
            !s.is_empty(),
            "the editor cache directory must resolve to a real path"
        ),
        other => panic!("expected a string path, got {other:?}"),
    }
}
