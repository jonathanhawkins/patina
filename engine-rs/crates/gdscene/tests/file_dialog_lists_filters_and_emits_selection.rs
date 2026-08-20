//! pat-s3rrv.162 acceptance: `FileDialog` lists a directory subject to its
//! settings and reports a loadable path when a selection is confirmed.
//!
//! The four settings interact, and each one alone is easy to get right while
//! the combination is wrong: `access` decides which root paths are expressed
//! against, `root_subfolder` confines browsing beneath it, `filters` hides
//! non-matching files, and `show_hidden_files` decides whether dotfiles appear.

use gdscene::file_dialog::{confirm_selection, list_entries, state_from_node, Access};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A fixture tree: two visible files of different types, a dotfile, and a
/// subfolder with its own file so `root_subfolder` has somewhere to confine to.
fn fixture() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("patina_file_dialog_{}_{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("art")).expect("fixture is creatable");
    std::fs::write(dir.join("icon.png"), b"p").unwrap();
    std::fs::write(dir.join("notes.txt"), b"t").unwrap();
    std::fs::write(dir.join(".secret.png"), b"h").unwrap();
    std::fs::write(dir.join("art/sprite.png"), b"s").unwrap();
    dir
}

/// A FileDialog node carrying the settings under test.
fn dialog(
    tree: &mut SceneTree,
    filters: &[&str],
    hidden: bool,
    subfolder: &str,
) -> gdscene::node::NodeId {
    let root = tree.root_id();
    let id = tree
        .add_child(root, Node::new("Picker", "FileDialog"))
        .expect("node is added");
    let node = tree.get_node_mut(id).expect("node exists");
    node.set_property(
        "filters",
        Variant::PackedStringArray(filters.iter().map(|s| (*s).to_string()).collect()),
    );
    node.set_property("show_hidden_files", Variant::Bool(hidden));
    node.set_property("access", Variant::Int(0));
    node.set_property("root_subfolder", Variant::String(subfolder.to_string()));
    id
}

/// Both classes exist with Godot's parents.
#[test]
fn file_dialog_and_editor_file_dialog_are_registered() {
    gdobject::class_db::register_all_classes();

    assert!(gdobject::class_db::class_exists("FileDialog"));
    assert!(gdobject::class_db::class_exists("EditorFileDialog"));
    // ⚠️ EditorFileDialog inherits FileDialog, not ConfirmationDialog — from
    // doc/classes/EditorFileDialog.xml. Parenting it to ConfirmationDialog
    // would still "exist" while losing every FileDialog member below it.
    assert!(
        gdobject::class_db::is_parent_class("EditorFileDialog", "FileDialog"),
        "EditorFileDialog must inherit FileDialog"
    );
    assert!(
        gdobject::class_db::is_parent_class("FileDialog", "ConfirmationDialog"),
        "FileDialog must inherit ConfirmationDialog"
    );

    // The members the acceptance's behaviour hangs off. Registering the class
    // without them satisfies `class_exists` while leaving reflection blind to
    // everything a dialog is configured with.
    for prop in [
        "filters",
        "show_hidden_files",
        "access",
        "root_subfolder",
        "current_dir",
        "current_path",
    ] {
        assert!(
            gdobject::class_db::class_has_property("FileDialog", prop),
            "FileDialog is missing the `{prop}` property"
        );
    }

    // Defaults, from doc/classes/FileDialog.xml — not guessed. `access` 0 is
    // ACCESS_RESOURCES, and a wrong default here would let a dialog browse the
    // whole filesystem by omission.
    assert_eq!(
        gdobject::class_db::property_get_revert("FileDialog", "access"),
        Some(Variant::Int(0))
    );
    assert_eq!(
        gdobject::class_db::property_get_revert("FileDialog", "show_hidden_files"),
        Some(Variant::Bool(false))
    );
    // ⚠️ `mode_overrides_title` defaults to TRUE, which is the one default here
    // that is not the type's zero value and so the one most likely to be
    // written as `false` by reflex.
    assert_eq!(
        gdobject::class_db::property_get_revert("FileDialog", "mode_overrides_title"),
        Some(Variant::Bool(true))
    );

    // ⚠️ Accessor names that cannot be derived from the property name. Godot
    // reports `is_showing_hidden_files`, not `get_show_hidden_files`; a script
    // calling the documented name would fail against a derived one.
    assert_eq!(
        gdobject::class_db::get_property_getter("FileDialog", "show_hidden_files"),
        "is_showing_hidden_files"
    );
    assert_eq!(
        gdobject::class_db::get_property_getter("FileDialog", "mode_overrides_title"),
        "is_mode_overriding_title"
    );
}

/// Filters select files; folders are listed regardless.
#[test]
fn filters_hide_non_matching_files_but_never_folders() {
    let dir = fixture();
    let mut tree = SceneTree::new();
    let id = dialog(&mut tree, &["*.png ; PNG Images"], false, "");
    let state = state_from_node(&tree, id);

    let names: Vec<String> = list_entries(&dir, &state)
        .into_iter()
        .map(|e| e.name)
        .collect();

    assert!(names.contains(&"icon.png".to_string()), "got {names:?}");
    assert!(
        !names.contains(&"notes.txt".to_string()),
        "a .txt passed a *.png filter: {names:?}"
    );
    // A filter selects files to OPEN; hiding folders would make their contents
    // unreachable, so `art` must survive a filter it cannot match.
    assert!(
        names.contains(&"art".to_string()),
        "the subfolder was filtered away: {names:?}"
    );
}

/// `show_hidden_files` gates dotfiles, independently of the filter.
#[test]
fn hidden_files_appear_only_when_asked_for() {
    let dir = fixture();
    let mut tree = SceneTree::new();

    let shown = dialog(&mut tree, &["*.png ; PNG Images"], false, "");
    let hidden_off = state_from_node(&tree, shown);
    let names: Vec<String> = list_entries(&dir, &hidden_off)
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert!(
        !names.contains(&".secret.png".to_string()),
        "a dotfile appeared with show_hidden_files off: {names:?}"
    );

    let all = dialog(&mut tree, &["*.png ; PNG Images"], true, "");
    let hidden_on = state_from_node(&tree, all);
    let names: Vec<String> = list_entries(&dir, &hidden_on)
        .into_iter()
        .map(|e| e.name)
        .collect();
    // ⚠️ It must still satisfy the filter: `show_hidden_files` reveals dotfiles,
    // it does not bypass filtering. A `.secret.txt` would stay hidden here.
    assert!(
        names.contains(&".secret.png".to_string()),
        "show_hidden_files did not reveal the dotfile: {names:?}"
    );
}

/// `root_subfolder` confines browsing, and the confirmed path includes it.
#[test]
fn root_subfolder_confines_browsing_and_qualifies_the_selection() {
    let dir = fixture();
    let mut tree = SceneTree::new();
    let id = dialog(&mut tree, &[], false, "art");
    let state = state_from_node(&tree, id);

    let root = gdscene::file_dialog::browse_root(&dir, &state);
    let names: Vec<String> = list_entries(&root, &state)
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(
        names,
        vec!["sprite.png".to_string()],
        "browsing should be confined to the subfolder"
    );
    assert_eq!(state.access, Access::Resources);
}

/// **The acceptance.** Confirming a selection emits `file_selected` carrying a
/// path a receiving script can actually load.
#[test]
fn confirming_a_selection_emits_file_selected_with_a_res_path() {
    let mut tree = SceneTree::new();
    let id = dialog(&mut tree, &[], false, "art");

    let root = tree.root_id();
    let listener = tree
        .add_child(root, Node::new("Listener", "Node"))
        .expect("listener is added");
    tree.attach_script(
        listener,
        Box::new(
            gdscene::GDScriptNodeInstance::from_source(
                "extends Node\nvar got = \"\"\nfunc _on_file_selected(path):\n\tgot = path\n",
                listener,
            )
            .expect("script parses"),
        ),
    );
    tree.connect_signal(
        id,
        "file_selected",
        gdobject::signal::Connection::new(listener.object_id(), "_on_file_selected"),
    );

    let returned = confirm_selection(&mut tree, id, "sprite.png");
    assert_eq!(returned, "res://art/sprite.png");

    // ⚠️ Asserting the RETURN value alone would pass even if the signal were
    // never emitted. The receiver is what proves delivery, and the payload it
    // sees is what a real script would `load()`.
    let seen = tree
        .get_script(listener)
        .expect("listener has a script")
        .get_property("got");
    assert_eq!(
        seen,
        Some(Variant::String("res://art/sprite.png".to_string())),
        "the connected receiver did not see the qualified path"
    );
}
