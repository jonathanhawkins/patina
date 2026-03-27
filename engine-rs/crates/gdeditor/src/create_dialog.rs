//! Node creation dialog with class search and filtering.
//!
//! Mirrors Godot's "Create New Node" dialog: lists registered classes from
//! [`ClassDB`](gdobject::class_db), supports text search, inheritance-based
//! filtering, favorites, and recently-used tracking.

use gdobject::class_db;

/// A single entry in the class list shown by the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassEntry {
    /// The class name (e.g., `"Sprite2D"`).
    pub class_name: String,
    /// The parent class name (empty for root `Object`).
    pub parent_class: String,
    /// Full inheritance chain from this class to `Object`.
    pub inheritance_chain: Vec<String>,
    /// Whether this class is marked as a user favorite.
    pub is_favorite: bool,
}

/// Filter mode for narrowing the class list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassFilter {
    /// No filter — show all classes.
    None,
    /// Only show classes that inherit from the given base class.
    InheritsFrom(String),
    /// Only show classes whose name contains the search string (case-insensitive).
    Search(String),
    /// Combined: must inherit from base AND match search text.
    SearchWithBase {
        search: String,
        base_class: String,
    },
}

/// Result of confirming a selection in the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateDialogResult {
    /// The class selected by the user.
    pub class_name: String,
    /// The parent class of the selected class.
    pub parent_class: String,
}

/// The node creation dialog.
///
/// Displays a searchable, filterable list of all registered classes from ClassDB.
/// Supports favorites, recent selections, and inheritance-based filtering.
#[derive(Debug)]
pub struct CreateNodeDialog {
    /// Current search/filter text.
    search_text: String,
    /// Base class filter (e.g., only show Node subclasses).
    base_class: Option<String>,
    /// The currently selected class name (if any).
    selected: Option<String>,
    /// User-favorited class names.
    favorites: Vec<String>,
    /// Recently selected class names (most recent first, capped).
    recent: Vec<String>,
    /// Maximum number of recent entries to keep.
    max_recent: usize,
    /// Whether the dialog is currently open/visible.
    visible: bool,
}

impl CreateNodeDialog {
    /// Creates a new dialog with default settings.
    pub fn new() -> Self {
        Self {
            search_text: String::new(),
            base_class: None,
            selected: None,
            favorites: Vec::new(),
            recent: Vec::new(),
            max_recent: 10,
            visible: false,
        }
    }

    /// Creates a new dialog restricted to subclasses of `base_class`.
    pub fn with_base_class(base_class: impl Into<String>) -> Self {
        Self {
            base_class: Some(base_class.into()),
            ..Self::new()
        }
    }

    /// Opens the dialog (makes it visible and clears the search).
    pub fn open(&mut self) {
        self.visible = true;
        self.search_text.clear();
        self.selected = None;
    }

    /// Closes the dialog.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Returns whether the dialog is currently open.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Sets the search/filter text.
    pub fn set_search(&mut self, text: impl Into<String>) {
        self.search_text = text.into();
    }

    /// Returns the current search text.
    pub fn search_text(&self) -> &str {
        &self.search_text
    }

    /// Sets the base class filter.
    pub fn set_base_class(&mut self, base: Option<String>) {
        self.base_class = base;
    }

    /// Returns the current base class filter.
    pub fn base_class(&self) -> Option<&str> {
        self.base_class.as_deref()
    }

    /// Selects a class by name. Returns `true` if the class exists in ClassDB.
    pub fn select(&mut self, class_name: &str) -> bool {
        if class_db::class_exists(class_name) {
            self.selected = Some(class_name.to_string());
            true
        } else {
            false
        }
    }

    /// Returns the currently selected class name, if any.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// Confirms the current selection and returns the result.
    ///
    /// Adds the selected class to the recent list and closes the dialog.
    /// Returns `None` if nothing is selected.
    pub fn confirm(&mut self) -> Option<CreateDialogResult> {
        let class_name = self.selected.clone()?;
        let info = class_db::get_class_info(&class_name)?;

        // Track in recent list (move to front, dedup, cap).
        self.recent.retain(|r| r != &class_name);
        self.recent.insert(0, class_name.clone());
        self.recent.truncate(self.max_recent);

        self.close();

        Some(CreateDialogResult {
            class_name,
            parent_class: info.parent_class,
        })
    }

    /// Adds a class to the favorites list. Returns `false` if already favorited.
    pub fn add_favorite(&mut self, class_name: impl Into<String>) -> bool {
        let name = class_name.into();
        if self.favorites.contains(&name) {
            return false;
        }
        self.favorites.push(name);
        true
    }

    /// Removes a class from the favorites list. Returns `true` if it was present.
    pub fn remove_favorite(&mut self, class_name: &str) -> bool {
        let len_before = self.favorites.len();
        self.favorites.retain(|f| f != class_name);
        self.favorites.len() < len_before
    }

    /// Returns the favorites list.
    pub fn favorites(&self) -> &[String] {
        &self.favorites
    }

    /// Returns the recently selected classes (most recent first).
    pub fn recent(&self) -> &[String] {
        &self.recent
    }

    /// Returns the active filter based on the current search text and base class.
    pub fn active_filter(&self) -> ClassFilter {
        let has_search = !self.search_text.is_empty();
        let has_base = self.base_class.is_some();

        match (has_search, has_base) {
            (false, false) => ClassFilter::None,
            (false, true) => {
                ClassFilter::InheritsFrom(self.base_class.clone().unwrap())
            }
            (true, false) => ClassFilter::Search(self.search_text.clone()),
            (true, true) => ClassFilter::SearchWithBase {
                search: self.search_text.clone(),
                base_class: self.base_class.clone().unwrap(),
            },
        }
    }

    /// Returns all classes matching the current filter, sorted alphabetically.
    ///
    /// Each entry includes the class name, parent, and inheritance chain.
    pub fn filtered_classes(&self) -> Vec<ClassEntry> {
        let all_classes = class_db::get_class_list();
        let filter = self.active_filter();

        let mut results: Vec<ClassEntry> = all_classes
            .into_iter()
            .filter(|name| matches_filter(name, &filter))
            .filter_map(|name| {
                let info = class_db::get_class_info(&name)?;
                let chain = class_db::inheritance_chain(&name);
                Some(ClassEntry {
                    class_name: name.clone(),
                    parent_class: info.parent_class.clone(),
                    inheritance_chain: chain,
                    is_favorite: self.favorites.contains(&name),
                })
            })
            .collect();

        // Sort: favorites first, then alphabetical.
        results.sort_by(|a, b| {
            b.is_favorite
                .cmp(&a.is_favorite)
                .then_with(|| a.class_name.cmp(&b.class_name))
        });

        results
    }

    /// Returns only favorite classes that match the current filter.
    pub fn filtered_favorites(&self) -> Vec<ClassEntry> {
        self.filtered_classes()
            .into_iter()
            .filter(|e| e.is_favorite)
            .collect()
    }

    /// Returns only recently used classes (unfiltered).
    pub fn recent_entries(&self) -> Vec<ClassEntry> {
        self.recent
            .iter()
            .filter_map(|name| {
                let info = class_db::get_class_info(name)?;
                let chain = class_db::inheritance_chain(name);
                Some(ClassEntry {
                    class_name: name.clone(),
                    parent_class: info.parent_class.clone(),
                    inheritance_chain: chain,
                    is_favorite: self.favorites.contains(name),
                })
            })
            .collect()
    }

    /// Returns the number of classes matching the current filter.
    pub fn match_count(&self) -> usize {
        let all = class_db::get_class_list();
        let filter = self.active_filter();
        all.iter()
            .filter(|name| matches_filter(name, &filter))
            .count()
    }
}

impl Default for CreateNodeDialog {
    fn default() -> Self {
        Self::new()
    }
}

/// Tests whether a class name passes the given filter.
fn matches_filter(class_name: &str, filter: &ClassFilter) -> bool {
    match filter {
        ClassFilter::None => true,
        ClassFilter::InheritsFrom(base) => {
            class_db::is_parent_class(class_name, base)
        }
        ClassFilter::Search(search) => {
            class_name.to_lowercase().contains(&search.to_lowercase())
        }
        ClassFilter::SearchWithBase { search, base_class } => {
            class_db::is_parent_class(class_name, base_class)
                && class_name.to_lowercase().contains(&search.to_lowercase())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdobject::class_db::{register_class, ClassRegistration};
    use std::sync::Mutex;

    // ClassDB is global so tests need serialization.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn setup() -> std::sync::MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock().unwrap();
        class_db::clear_for_testing();

        // Register a small class hierarchy:
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

    #[test]
    fn new_dialog_defaults() {
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
    fn open_and_close() {
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
        d.open();
        assert!(d.search_text().is_empty());
        assert!(d.selected().is_none());
    }

    #[test]
    fn with_base_class() {
        let _g = setup();
        let d = CreateNodeDialog::with_base_class("Node");
        assert_eq!(d.base_class(), Some("Node"));
    }

    #[test]
    fn select_valid_class() {
        let _g = setup();
        let mut d = CreateNodeDialog::new();
        assert!(d.select("Sprite2D"));
        assert_eq!(d.selected(), Some("Sprite2D"));
    }

    #[test]
    fn select_invalid_class_returns_false() {
        let _g = setup();
        let mut d = CreateNodeDialog::new();
        assert!(!d.select("DoesNotExist"));
        assert!(d.selected().is_none());
    }

    #[test]
    fn unfiltered_returns_all_classes() {
        let _g = setup();
        let d = CreateNodeDialog::new();
        let classes = d.filtered_classes();
        assert_eq!(classes.len(), 11);
    }

    #[test]
    fn filter_by_search_text() {
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
    fn filter_by_search_case_insensitive() {
        let _g = setup();
        let mut d = CreateNodeDialog::new();
        d.set_search("BUTTON");
        let classes = d.filtered_classes();
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].class_name, "Button");
    }

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
    fn filter_combined_search_and_base() {
        let _g = setup();
        let mut d = CreateNodeDialog::new();
        d.set_base_class(Some("Node".to_string()));
        d.set_search("3D");
        let classes = d.filtered_classes();
        let names: Vec<&str> = classes.iter().map(|c| c.class_name.as_str()).collect();
        assert!(names.contains(&"Node3D"));
        assert!(names.contains(&"MeshInstance3D"));
        assert!(!names.contains(&"Node2D")); // has "2D" not "3D"
    }

    #[test]
    fn favorites_add_remove() {
        let _g = setup();
        let mut d = CreateNodeDialog::new();
        assert!(d.add_favorite("Sprite2D"));
        assert!(!d.add_favorite("Sprite2D")); // duplicate
        assert_eq!(d.favorites().len(), 1);
        assert!(d.remove_favorite("Sprite2D"));
        assert!(!d.remove_favorite("Sprite2D")); // already removed
        assert!(d.favorites().is_empty());
    }

    #[test]
    fn favorites_sort_first() {
        let _g = setup();
        let mut d = CreateNodeDialog::new();
        d.add_favorite("Sprite2D");
        let classes = d.filtered_classes();
        assert_eq!(classes[0].class_name, "Sprite2D");
        assert!(classes[0].is_favorite);
        assert!(!classes[1].is_favorite);
    }

    #[test]
    fn confirm_returns_result_and_tracks_recent() {
        let _g = setup();
        let mut d = CreateNodeDialog::new();
        d.open();
        d.select("Button");
        let result = d.confirm().unwrap();
        assert_eq!(result.class_name, "Button");
        assert_eq!(result.parent_class, "Control");
        assert!(!d.is_visible()); // closed after confirm
        assert_eq!(d.recent(), &["Button"]);
    }

    #[test]
    fn confirm_without_selection_returns_none() {
        let _g = setup();
        let mut d = CreateNodeDialog::new();
        d.open();
        assert!(d.confirm().is_none());
    }

    #[test]
    fn recent_deduplicates_and_caps() {
        let _g = setup();
        let mut d = CreateNodeDialog::new();
        d.max_recent = 3;

        // Select and confirm several classes.
        for name in &["Label", "Button", "Sprite2D", "Node2D"] {
            d.open();
            d.select(name);
            d.confirm();
        }
        // Most recent first, capped at 3.
        assert_eq!(d.recent(), &["Node2D", "Sprite2D", "Button"]);

        // Re-selecting Button moves it to front.
        d.open();
        d.select("Button");
        d.confirm();
        assert_eq!(d.recent(), &["Button", "Node2D", "Sprite2D"]);
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
        assert!(entries[0].inheritance_chain.contains(&"Node".to_string()));
    }

    #[test]
    fn inheritance_chain_populated() {
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
    fn match_count_reflects_filter() {
        let _g = setup();
        let mut d = CreateNodeDialog::new();
        assert_eq!(d.match_count(), 11);
        d.set_search("node");
        // "Node", "Node2D", "Node3D" match
        assert_eq!(d.match_count(), 3);
    }

    #[test]
    fn active_filter_variants() {
        let _g = setup();
        let mut d = CreateNodeDialog::new();
        assert_eq!(d.active_filter(), ClassFilter::None);

        d.set_search("test");
        assert_eq!(d.active_filter(), ClassFilter::Search("test".to_string()));

        d.set_search("");
        d.set_base_class(Some("Node".to_string()));
        assert_eq!(
            d.active_filter(),
            ClassFilter::InheritsFrom("Node".to_string())
        );

        d.set_search("2D");
        assert_eq!(
            d.active_filter(),
            ClassFilter::SearchWithBase {
                search: "2D".to_string(),
                base_class: "Node".to_string(),
            }
        );
    }

    #[test]
    fn filtered_favorites_only_matching() {
        let _g = setup();
        let mut d = CreateNodeDialog::new();
        d.add_favorite("Sprite2D");
        d.add_favorite("Button");
        d.set_search("sprite");
        let favs = d.filtered_favorites();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].class_name, "Sprite2D");
    }

    #[test]
    fn default_trait_impl() {
        let _g = setup();
        let d = CreateNodeDialog::default();
        assert!(!d.is_visible());
        assert_eq!(d.filtered_classes().len(), 11);
    }

    #[test]
    fn empty_search_matches_all() {
        let _g = setup();
        let mut d = CreateNodeDialog::new();
        d.set_search("");
        assert_eq!(d.filtered_classes().len(), 11);
    }

    #[test]
    fn set_base_class_none_removes_filter() {
        let _g = setup();
        let mut d = CreateNodeDialog::with_base_class("Control");
        assert_eq!(d.filtered_classes().len(), 3);
        d.set_base_class(None);
        assert_eq!(d.filtered_classes().len(), 11);
    }
}
