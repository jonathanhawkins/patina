//! pat-mgixk.54 acceptance (part 2) — a booted project's script reaches the
//! DirAccess surface.
//!
//! Godot's `DirAccess` (`core/io/dir_access.cpp`, `doc/classes/DirAccess.xml`)
//! carries `make_dir_recursive`, `copy`, `change_dir`, `get_current_dir`,
//! `file_exists` and the `*_absolute` static family. gdcore implemented nearly
//! all of it (`file_access.rs:1418-1557`), but the script binding dispatched
//! only 11 methods — so `DirAccess.make_dir_recursive_absolute("user://saves/slot1")`,
//! the standard save-folder idiom, raised `UndefinedFunction`.
//!
//! # Why these assertions
//!
//! ⚠️ The directories are checked on DISK from Rust as well as through
//! `dir_exists_absolute` from script. A binding that returned a plausible value
//! without touching the filesystem would satisfy the script-side check alone;
//! only the on-disk check proves the call reached `fs::create_dir_all`.
//!
//! ⚠️ `get_files_at` is asserted to find exactly ONE entry. The fixture puts a
//! directory (`c`) and a file (`y.txt`) in the same folder, so a binding that
//! listed directories too would return 2. Asserting only `>= 1` would accept
//! that.
//!
//! ⚠️ The project is booted with an explicit `user://` root.
//! `boot_project` leaves `user://` unmapped (`project_boot.rs:91-95`), so every
//! call here would fail for a reason that has nothing to do with the bindings.

use std::path::{Path, PathBuf};

use gdscene::{LifecycleManager, NodeId, SceneTree};
use gdvariant::Variant;
use patina_runner::project_boot::boot_project_with_user_root;

fn fixture_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/diraccess_surface"
    ))
}

/// A private `user://` root for this test process. `std::env::temp_dir` rather
/// than a tempdir crate: patina-runner declares no dev-dependencies.
fn user_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("patina_diraccess_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("user:// root must be creatable");
    dir
}

fn boot(user_dir: &Path) -> (SceneTree, NodeId) {
    let booted = boot_project_with_user_root(&fixture_dir(), Some(user_dir.to_path_buf()))
        .expect("the fixture project must boot");
    let mut tree = booted.tree;
    let main_root = booted
        .main_scene_root
        .expect("the project declares run/main_scene, so it must instance");
    LifecycleManager::enter_tree(&mut tree, main_root);
    (tree, main_root)
}

fn script_var(tree: &SceneTree, id: NodeId, name: &str) -> Variant {
    tree.get_script(id)
        .unwrap_or_else(|| panic!("the main scene must have its script attached to read `{name}`"))
        .get_property(name)
        .unwrap_or_else(|| panic!("the script must declare `{name}`"))
}

fn as_int(v: &Variant, name: &str) -> i64 {
    match v {
        Variant::Int(i) => *i,
        other => panic!("`{name}` should be an Int, got {other:?}"),
    }
}

fn as_str(v: &Variant, name: &str) -> String {
    match v {
        Variant::String(s) => s.clone(),
        other => panic!("`{name}` should be a String, got {other:?}"),
    }
}

/// **The acceptance test.**
#[test]
fn diraccess_script_surface_wired() {
    let user_dir = user_root();
    let (tree, main) = boot(&user_dir);
    let var = |n: &str| script_var(&tree, main, n);

    // -- fixture check first ------------------------------------------------
    //
    // `reached_end` is the last line of `_ready`. Before this bead the very
    // first call (`make_dir_recursive_absolute`) raised, so `_ready` died there
    // and every variable below kept its declared default — which is exactly
    // what a wrong return value would look like.
    assert_eq!(
        var("reached_end"),
        Variant::Bool(true),
        "`_ready` must have run to completion. False means one of the DirAccess \
         calls raised UndefinedFunction and aborted the script, which is the \
         pat-mgixk.54 bug itself."
    );

    // -- make_dir_recursive_absolute actually created the tree --------------
    assert_eq!(
        as_int(&var("made"), "made"),
        0,
        "`make_dir_recursive_absolute` must report OK (0), as Godot's Error does"
    );
    assert_eq!(
        var("deep_exists"),
        Variant::Bool(true),
        "`dir_exists_absolute(\"user://a/b/c\")` must see the tree the previous \
         call created"
    );
    // The independent check: the script's own answer could be wrong in the same
    // direction as the call that produced it.
    assert!(
        user_dir.join("a/b/c").is_dir(),
        "the nested directories must exist ON DISK at {}. Missing means the \
         binding answered without reaching fs::create_dir_all.",
        user_dir.join("a/b/c").display()
    );

    // -- copy, through a handle opened on user://a --------------------------
    assert_eq!(
        as_int(&var("copy_ret"), "copy_ret"),
        0,
        "`DirAccess.copy` must report OK (0)"
    );
    assert!(
        user_dir.join("a/b/y.txt").is_file(),
        "the copied file must exist on disk at {}",
        user_dir.join("a/b/y.txt").display()
    );

    // -- get_files_at lists files, and ONLY files ---------------------------
    assert_eq!(
        as_int(&var("files_n"), "files_n"),
        1,
        "`get_files_at(\"user://a/b\")` must find exactly the one file. The \
         folder also holds the directory `c`, so 2 would mean directories are \
         being listed as files, and 0 means the listing never reached the \
         filesystem."
    );
    assert_eq!(
        as_str(&var("files_first"), "files_first"),
        "y.txt",
        "the listed file must be the copied one, by name"
    );

    // -- change_dir moved the handle ----------------------------------------
    //
    // A suffix match, not the whole path: the `user://` root is a temp
    // directory this test does not want to hardcode. `get_current_dir` returns
    // the RESOLVED filesystem path (file_access.rs:1436 returns `self.path`),
    // not the `user://` spelling.
    let cur = as_str(&var("cur_dir"), "cur_dir");
    assert!(
        cur.ends_with("/a/b"),
        "after `change_dir(\"b\")` from a handle on `user://a`, \
         `get_current_dir()` must end with /a/b; got {cur:?}. The unchanged \
         parent would end with /a, which is what a no-op change_dir looks like."
    );
    assert_eq!(
        var("sub_exists"),
        Variant::Bool(true),
        "`dir_exists(\"c\")` must resolve against the handle's NEW directory. \
         False means the bare name was looked up against the old directory (or \
         the process working directory) instead of `b` — the reason Godot's \
         instance `dir_exists` is not simply the static one."
    );

    // -- open() reports failure by returning null ---------------------------
    assert_eq!(
        var("nope_is_null"),
        Variant::Bool(true),
        "`DirAccess.open` on a missing directory must return null so the script \
         can test it, rather than raising and killing the script"
    );
    assert_eq!(
        as_int(&var("nope_err"), "nope_err"),
        7,
        "`DirAccess.get_open_error()` must answer the Godot Error code \
         (7 = ERR_FILE_NOT_FOUND), which is what a script compares against. A \
         message String is never `==` to the bareword."
    );

    let _ = std::fs::remove_dir_all(&user_dir);
}
