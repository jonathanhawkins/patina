//! pat-l6qzk: Node creation dialog with class search and filtering.
//!
//! Validates:
//! 1. CreateNodeDialog defaults and open/close lifecycle
//! 2. Search filtering (case-insensitive substring match)
//! 3. Base class inheritance filtering
//! 4. Combined search + base class filtering
//! 5. Favorites: add/remove, sorting (favorites first)
//! 6. Recent selections: deduplication, capping, ordering
//! 7. Confirm returns result with parent class and tracks recent
//! 8. ClassEntry inheritance chain correctness
//! 9. ClassFilter variants from active_filter()
//! 10. Match count reflects current filter
//! 11. Filtered favorites intersect favorites with search
//! 12. Open clears search and selection state

use gdeditor::create_dialog::{ClassFilter, CreateNodeDialog};
use gdobject::class_db::{self, register_class, ClassRegistration};
use std::sync::Mutex;

// ClassDB is a global singleton — serialize tests.
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().unwrap();
    class_db::clear_for_testing();

    // Register a class hierarchy:
    //   Object
    //   ├── Node
    //   │   ├── Node2D
    //   │   │   ├── Sprite2D
    //   │   │   └── AnimatedSprite2D
    //   │   ├── Node3D
    //   │   │   └── MeshInstance3D
    //   │   └── Control
    //   │       ├── Button
    //   │       └── Label
    //   └── Resource
    register_class(ClassRegistration::new("Object"));
    register_class(ClassRegistration::new("Node").parent("Object"));
    register_class(ClassRegistration::new("Node2D").parent("Node"));
    register_class(ClassRegistration::new("Sprite2D").parent("Node2D"));
    register_class(ClassRegistration::new("AnimatedSprite2D").parent("Node2D"));
    register_class(ClassRegistration::new("Node3D").parent("Node"));
    register_class(ClassRegistration::new("MeshInstance3D").parent("Node3D"));
    register_class(ClassRegistration::new("Control").parent("Node"));
    register_class(ClassRegistration::new("Button").parent("Control"));
    register_class(ClassRegistration::new("Label").parent("Control"));
    register_class(ClassRegistration::new("Resource").parent("Object"));

    guard
}

// ── Lifecycle ───────────────────────────────────────────────────────

#[test]
fn dialog_defaults() {
    let _g = setup();
    let d = CreateNodeDialog::new();
    assert!(!d.is_visible());
    assert!(d.selected().is_none());
    assert!(d.search_text().is_empty());
    assert!(d.base_class().is_none());
    assert!(d.favorites().is_empty());
    assert!(d.recent().is_empty());
}

#[test]
fn dialog_open_close() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.open();
    assert!(d.is_visible());
    d.close();
    assert!(!d.is_visible());
}

#[test]
fn open_clears_search_and_selection() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.set_search("Node");
    d.select("Sprite2D");
    assert_eq!(d.selected(), Some("Sprite2D"));
    d.open();
    assert!(d.search_text().is_empty());
    assert!(d.selected().is_none());
}

#[test]
fn with_base_class_constructor() {
    let _g = setup();
    let d = CreateNodeDialog::with_base_class("Control");
    assert_eq!(d.base_class(), Some("Control"));
}

// ── Selection ───────────────────────────────────────────────────────

#[test]
fn select_valid_class() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    assert!(d.select("Sprite2D"));
    assert_eq!(d.selected(), Some("Sprite2D"));
}

#[test]
fn select_invalid_class_fails() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    assert!(!d.select("NonexistentClass"));
    assert!(d.selected().is_none());
}

// ── Search filtering ────────────────────────────────────────────────

#[test]
fn search_filters_by_substring() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.set_search("sprite");
    let classes = d.filtered_classes();
    let names: Vec<&str> = classes.iter().map(|c| c.class_name.as_str()).collect();
    assert!(names.contains(&"Sprite2D"));
    assert!(names.contains(&"AnimatedSprite2D"));
    assert_eq!(names.len(), 2);
}

#[test]
fn search_is_case_insensitive() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.set_search("BUTTON");
    let classes = d.filtered_classes();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].class_name, "Button");
}

#[test]
fn empty_search_matches_all() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.set_search("");
    assert_eq!(d.filtered_classes().len(), 11);
}

#[test]
fn no_match_returns_empty() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.set_search("zzzzzzz");
    assert!(d.filtered_classes().is_empty());
    assert_eq!(d.match_count(), 0);
}

// ── Base class filtering ────────────────────────────────────────────

#[test]
fn filter_by_base_class() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.set_base_class(Some("Control".to_string()));
    let classes = d.filtered_classes();
    let names: Vec<&str> = classes.iter().map(|c| c.class_name.as_str()).collect();
    assert!(names.contains(&"Control"));
    assert!(names.contains(&"Button"));
    assert!(names.contains(&"Label"));
    assert_eq!(names.len(), 3);
}

#[test]
fn remove_base_class_shows_all() {
    let _g = setup();
    let mut d = CreateNodeDialog::with_base_class("Control");
    assert_eq!(d.filtered_classes().len(), 3);
    d.set_base_class(None);
    assert_eq!(d.filtered_classes().len(), 11);
}

// ── Combined filtering ──────────────────────────────────────────────

#[test]
fn combined_search_and_base_class() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.set_base_class(Some("Node".to_string()));
    d.set_search("3D");
    let classes = d.filtered_classes();
    let names: Vec<&str> = classes.iter().map(|c| c.class_name.as_str()).collect();
    assert!(names.contains(&"Node3D"));
    assert!(names.contains(&"MeshInstance3D"));
    assert!(!names.contains(&"Node2D"));
}

// ── Favorites ───────────────────────────────────────────────────────

#[test]
fn favorites_add_and_remove() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    assert!(d.add_favorite("Sprite2D"));
    assert!(!d.add_favorite("Sprite2D")); // duplicate rejected
    assert_eq!(d.favorites().len(), 1);
    assert!(d.remove_favorite("Sprite2D"));
    assert!(!d.remove_favorite("Sprite2D")); // already gone
    assert!(d.favorites().is_empty());
}

#[test]
fn favorites_sorted_first() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.add_favorite("Sprite2D");
    let classes = d.filtered_classes();
    assert_eq!(classes[0].class_name, "Sprite2D");
    assert!(classes[0].is_favorite);
    assert!(!classes[1].is_favorite);
}

#[test]
fn filtered_favorites_intersects_with_search() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.add_favorite("Sprite2D");
    d.add_favorite("Button");
    d.set_search("sprite");
    let favs = d.filtered_favorites();
    assert_eq!(favs.len(), 1);
    assert_eq!(favs[0].class_name, "Sprite2D");
}

// ── Confirm and recent ──────────────────────────────────────────────

#[test]
fn confirm_returns_result_and_closes() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.open();
    d.select("Button");
    let result = d.confirm().unwrap();
    assert_eq!(result.class_name, "Button");
    assert_eq!(result.parent_class, "Control");
    assert!(!d.is_visible());
}

#[test]
fn confirm_tracks_recent() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.open();
    d.select("Label");
    d.confirm();
    assert_eq!(d.recent(), &["Label"]);
}

#[test]
fn confirm_without_selection_returns_none() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.open();
    assert!(d.confirm().is_none());
}

#[test]
fn recent_deduplicates() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    for name in &["Label", "Button", "Label"] {
        d.open();
        d.select(name);
        d.confirm();
    }
    // Label re-selected moves to front, no duplicate
    assert_eq!(d.recent()[0], "Label");
    assert_eq!(d.recent()[1], "Button");
    // No duplicate of Label
    assert_eq!(d.recent().iter().filter(|r| *r == "Label").count(), 1);
}

#[test]
fn recent_reselection_moves_to_front() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    for name in &["Label", "Button", "Sprite2D"] {
        d.open();
        d.select(name);
        d.confirm();
    }
    // Sprite2D is most recent
    assert_eq!(d.recent()[0], "Sprite2D");
    // Re-select Label
    d.open();
    d.select("Label");
    d.confirm();
    assert_eq!(d.recent()[0], "Label");
}

#[test]
fn recent_entries_returns_class_entries() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.open();
    d.select("MeshInstance3D");
    d.confirm();
    let entries = d.recent_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].class_name, "MeshInstance3D");
    assert_eq!(entries[0].parent_class, "Node3D");
}

// ── Inheritance chain ───────────────────────────────────────────────

#[test]
fn inheritance_chain_correct() {
    let _g = setup();
    let d = CreateNodeDialog::new();
    let classes = d.filtered_classes();
    let sprite = classes.iter().find(|c| c.class_name == "Sprite2D").unwrap();
    assert_eq!(
        sprite.inheritance_chain,
        vec!["Sprite2D", "Node2D", "Node", "Object"]
    );
}

#[test]
fn root_class_chain_is_just_itself() {
    let _g = setup();
    let d = CreateNodeDialog::new();
    let classes = d.filtered_classes();
    let obj = classes.iter().find(|c| c.class_name == "Object").unwrap();
    assert_eq!(obj.inheritance_chain, vec!["Object"]);
}

// ── ClassFilter variants ────────────────────────────────────────────

#[test]
fn active_filter_none_by_default() {
    let _g = setup();
    let d = CreateNodeDialog::new();
    assert_eq!(d.active_filter(), ClassFilter::None);
}

#[test]
fn active_filter_search_only() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.set_search("test");
    assert_eq!(d.active_filter(), ClassFilter::Search("test".to_string()));
}

#[test]
fn active_filter_base_only() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.set_base_class(Some("Node".to_string()));
    assert_eq!(d.active_filter(), ClassFilter::InheritsFrom("Node".to_string()));
}

#[test]
fn active_filter_combined() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.set_base_class(Some("Node".to_string()));
    d.set_search("2D");
    assert_eq!(
        d.active_filter(),
        ClassFilter::SearchWithBase {
            search: "2D".to_string(),
            base_class: "Node".to_string(),
        }
    );
}

// ── Match count ─────────────────────────────────────────────────────

#[test]
fn match_count_all() {
    let _g = setup();
    let d = CreateNodeDialog::new();
    assert_eq!(d.match_count(), 11);
}

#[test]
fn match_count_with_search() {
    let _g = setup();
    let mut d = CreateNodeDialog::new();
    d.set_search("node");
    assert_eq!(d.match_count(), 3); // Node, Node2D, Node3D
}

// ── Default trait ───────────────────────────────────────────────────

#[test]
fn default_trait_works() {
    let _g = setup();
    let d = CreateNodeDialog::default();
    assert!(!d.is_visible());
    assert_eq!(d.filtered_classes().len(), 11);
}
