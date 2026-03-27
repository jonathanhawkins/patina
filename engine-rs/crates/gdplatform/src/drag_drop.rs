//! Drag-and-drop file handling.
//!
//! Provides a platform-agnostic drag-and-drop interface mirroring Godot's
//! file drop and internal drag-and-drop support:
//!
//! - **OS file drops**: Files dragged from the desktop onto a window
//!   (`Window.files_dropped` signal in Godot).
//! - **Internal drag data**: Godot-style `_get_drag_data` / `_can_drop_data` /
//!   `_drop_data` for in-engine UI drag-and-drop.
//! - **File filtering**: Extension and path-based filtering for accepted file
//!   types (e.g., only `.tscn`, `.tres`, `.gd` files).
//!
//! The [`HeadlessDrop`] implementation stores state in-memory for testing.
//! Platform backends can implement [`DropHandler`] for native drag integration.

use std::collections::HashSet;
use std::path::Path;

use crate::input::DropContext;

// ---------------------------------------------------------------------------
// DropData — internal drag payload
// ---------------------------------------------------------------------------

/// Payload carried during an internal (in-engine) drag-and-drop operation.
///
/// Mirrors Godot's `Variant` drag data returned by `_get_drag_data()`.
#[derive(Debug, Clone, PartialEq)]
pub enum DropData {
    /// No active drag.
    None,
    /// A list of file/resource paths being dragged.
    Files(Vec<String>),
    /// A dictionary-style payload (key-value pairs).
    Dictionary(Vec<(String, String)>),
    /// A plain text payload.
    Text(String),
    /// An integer payload (e.g., list index being dragged).
    Index(i64),
}

impl DropData {
    /// Returns `true` if there is no active drag data.
    pub fn is_none(&self) -> bool {
        matches!(self, DropData::None)
    }

    /// Returns the file paths if this is a `Files` payload.
    pub fn as_files(&self) -> Option<&[String]> {
        match self {
            DropData::Files(f) => Some(f),
            _ => Option::None,
        }
    }

    /// Returns the text if this is a `Text` payload.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            DropData::Text(t) => Some(t),
            _ => Option::None,
        }
    }

    /// Returns the index if this is an `Index` payload.
    pub fn as_index(&self) -> Option<i64> {
        match self {
            DropData::Index(i) => Some(*i),
            _ => Option::None,
        }
    }
}

// ---------------------------------------------------------------------------
// DropFilter — file extension filter
// ---------------------------------------------------------------------------

/// Filters dropped files by extension.
///
/// Used to accept only specific file types during a drag-and-drop operation
/// (e.g., an editor panel that only accepts `.tscn` scene files).
#[derive(Debug, Clone)]
pub struct DropFilter {
    /// Allowed extensions (lowercase, without leading dot).
    extensions: HashSet<String>,
    /// Whether to accept directories.
    accept_directories: bool,
}

impl DropFilter {
    /// Creates a filter that accepts all files.
    pub fn accept_all() -> Self {
        Self {
            extensions: HashSet::new(),
            accept_directories: true,
        }
    }

    /// Creates a filter for specific extensions (without leading dot).
    ///
    /// ```ignore
    /// let filter = DropFilter::extensions(&["tscn", "tres", "gd"]);
    /// ```
    pub fn extensions(exts: &[&str]) -> Self {
        Self {
            extensions: exts.iter().map(|e| e.to_lowercase()).collect(),
            accept_directories: false,
        }
    }

    /// Sets whether directories are accepted.
    pub fn with_directories(mut self, accept: bool) -> Self {
        self.accept_directories = accept;
        self
    }

    /// Returns `true` if the given path passes the filter.
    pub fn accepts(&self, path: &str) -> bool {
        // Accept-all: no extensions specified and directories allowed.
        if self.extensions.is_empty() && self.accept_directories {
            return true;
        }

        let p = Path::new(path);

        // Check if it looks like a directory (trailing slash or no extension).
        if path.ends_with('/') || path.ends_with('\\') {
            return self.accept_directories;
        }

        // Check extension.
        if self.extensions.is_empty() {
            // No extension filter → accept all files.
            return true;
        }

        match p.extension().and_then(|e| e.to_str()) {
            Some(ext) => self.extensions.contains(&ext.to_lowercase()),
            // No extension — only accept if we accept directories and it could be one.
            None => self.accept_directories,
        }
    }

    /// Filters a list of paths, returning only those that pass.
    pub fn filter_paths<'a>(&self, paths: &'a [String]) -> Vec<&'a str> {
        paths.iter().filter(|p| self.accepts(p)).map(|p| p.as_str()).collect()
    }
}

// ---------------------------------------------------------------------------
// DropHandler trait
// ---------------------------------------------------------------------------

/// Platform-agnostic drag-and-drop handler.
///
/// Combines OS-level file drops (from the desktop) with Godot-style internal
/// drag-and-drop for in-engine UI operations.
pub trait DropHandler {
    /// Returns a reference to the OS file drop context.
    fn drop_context(&self) -> &DropContext;

    /// Returns a mutable reference to the OS file drop context.
    fn drop_context_mut(&mut self) -> &mut DropContext;

    /// Returns the current internal drag data (Godot-style `_get_drag_data`).
    fn get_drag_data(&self) -> &DropData;

    /// Starts an internal drag operation with the given payload.
    fn start_drag(&mut self, data: DropData);

    /// Tests whether the target can accept the current drag data.
    /// Mirrors Godot's `_can_drop_data()`.
    fn can_drop(&self, at_position: (f32, f32)) -> bool;

    /// Completes the internal drag, delivering the data.
    /// Mirrors Godot's `_drop_data()`. Returns the delivered data.
    fn accept_drop(&mut self, at_position: (f32, f32)) -> DropData;

    /// Cancels the current internal drag operation.
    fn cancel_drag(&mut self);

    /// Returns `true` if an internal drag is in progress.
    fn is_dragging(&self) -> bool;
}

// ---------------------------------------------------------------------------
// HeadlessDrop — in-memory implementation for testing
// ---------------------------------------------------------------------------

/// In-memory drag-and-drop handler for testing and headless mode.
#[derive(Debug, Clone)]
pub struct HeadlessDrop {
    context: DropContext,
    drag_data: DropData,
    /// Optional filter for `can_drop` checks on file drops.
    filter: Option<DropFilter>,
    /// Drop zone rectangle: (x, y, width, height). If `None`, the entire
    /// window is a valid drop target.
    drop_zone: Option<(f32, f32, f32, f32)>,
}

impl HeadlessDrop {
    /// Creates a new headless drop handler.
    pub fn new() -> Self {
        Self {
            context: DropContext::new(),
            drag_data: DropData::None,
            filter: None,
            drop_zone: None,
        }
    }

    /// Creates a handler with a file extension filter.
    pub fn with_filter(filter: DropFilter) -> Self {
        Self {
            context: DropContext::new(),
            drag_data: DropData::None,
            filter: Some(filter),
            drop_zone: None,
        }
    }

    /// Sets a rectangular drop zone. Drops outside this zone are rejected.
    pub fn set_drop_zone(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.drop_zone = Some((x, y, width, height));
    }

    /// Clears the drop zone, making the entire window a valid target.
    pub fn clear_drop_zone(&mut self) {
        self.drop_zone = None;
    }

    /// Returns the configured filter, if any.
    pub fn filter(&self) -> Option<&DropFilter> {
        self.filter.as_ref()
    }

    fn position_in_zone(&self, pos: (f32, f32)) -> bool {
        match self.drop_zone {
            None => true,
            Some((x, y, w, h)) => {
                pos.0 >= x && pos.0 <= x + w && pos.1 >= y && pos.1 <= y + h
            }
        }
    }
}

impl Default for HeadlessDrop {
    fn default() -> Self {
        Self::new()
    }
}

impl DropHandler for HeadlessDrop {
    fn drop_context(&self) -> &DropContext {
        &self.context
    }

    fn drop_context_mut(&mut self) -> &mut DropContext {
        &mut self.context
    }

    fn get_drag_data(&self) -> &DropData {
        &self.drag_data
    }

    fn start_drag(&mut self, data: DropData) {
        self.drag_data = data;
    }

    fn can_drop(&self, at_position: (f32, f32)) -> bool {
        if self.drag_data.is_none() {
            return false;
        }
        if !self.position_in_zone(at_position) {
            return false;
        }
        // If we have a filter and the drag data is files, check extensions.
        if let (Some(filter), DropData::Files(paths)) = (&self.filter, &self.drag_data) {
            let accepted = filter.filter_paths(paths);
            return !accepted.is_empty();
        }
        true
    }

    fn accept_drop(&mut self, at_position: (f32, f32)) -> DropData {
        if !self.can_drop(at_position) {
            return DropData::None;
        }
        std::mem::replace(&mut self.drag_data, DropData::None)
    }

    fn cancel_drag(&mut self) {
        self.drag_data = DropData::None;
    }

    fn is_dragging(&self) -> bool {
        !self.drag_data.is_none()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- DropData ----------------------------------------------------------

    #[test]
    fn drop_data_none() {
        let d = DropData::None;
        assert!(d.is_none());
        assert!(d.as_files().is_none());
        assert!(d.as_text().is_none());
        assert!(d.as_index().is_none());
    }

    #[test]
    fn drop_data_files() {
        let d = DropData::Files(vec!["a.tscn".into(), "b.gd".into()]);
        assert!(!d.is_none());
        assert_eq!(d.as_files().unwrap().len(), 2);
    }

    #[test]
    fn drop_data_text() {
        let d = DropData::Text("hello".into());
        assert_eq!(d.as_text(), Some("hello"));
        assert!(d.as_files().is_none());
    }

    #[test]
    fn drop_data_index() {
        let d = DropData::Index(42);
        assert_eq!(d.as_index(), Some(42));
    }

    // -- DropFilter --------------------------------------------------------

    #[test]
    fn filter_accept_all() {
        let f = DropFilter::accept_all();
        assert!(f.accepts("anything.txt"));
        assert!(f.accepts("scene.tscn"));
        assert!(f.accepts("/some/dir/"));
    }

    #[test]
    fn filter_by_extension() {
        let f = DropFilter::extensions(&["tscn", "tres"]);
        assert!(f.accepts("level.tscn"));
        assert!(f.accepts("material.tres"));
        assert!(!f.accepts("script.gd"));
        assert!(!f.accepts("readme.md"));
    }

    #[test]
    fn filter_case_insensitive() {
        let f = DropFilter::extensions(&["png"]);
        assert!(f.accepts("image.PNG"));
        assert!(f.accepts("image.Png"));
        assert!(f.accepts("image.png"));
    }

    #[test]
    fn filter_directories() {
        let f = DropFilter::extensions(&["tscn"]).with_directories(true);
        assert!(f.accepts("level.tscn"));
        assert!(f.accepts("/some/dir/"));
    }

    #[test]
    fn filter_paths() {
        let f = DropFilter::extensions(&["gd", "tscn"]);
        let paths = vec![
            "script.gd".into(),
            "readme.md".into(),
            "scene.tscn".into(),
            "image.png".into(),
        ];
        let accepted = f.filter_paths(&paths);
        assert_eq!(accepted, vec!["script.gd", "scene.tscn"]);
    }

    // -- HeadlessDrop basic -------------------------------------------------

    #[test]
    fn headless_starts_idle() {
        let h = HeadlessDrop::new();
        assert!(!h.is_dragging());
        assert!(h.get_drag_data().is_none());
        assert!(!h.drop_context().is_hovering());
        assert!(!h.drop_context().has_pending_files());
    }

    #[test]
    fn headless_os_file_drop() {
        let mut h = HeadlessDrop::new();
        h.drop_context_mut().drag_enter();
        assert!(h.drop_context().is_hovering());

        h.drop_context_mut().drop_files(vec![
            "/tmp/scene.tscn".into(),
            "/tmp/script.gd".into(),
        ]);
        assert!(!h.drop_context().is_hovering());
        assert!(h.drop_context().has_pending_files());
        assert_eq!(h.drop_context().pending_count(), 2);

        let files = h.drop_context_mut().take_pending_files();
        assert_eq!(files, vec!["/tmp/scene.tscn", "/tmp/script.gd"]);
        assert!(!h.drop_context().has_pending_files());
    }

    #[test]
    fn headless_internal_drag_lifecycle() {
        let mut h = HeadlessDrop::new();
        assert!(!h.is_dragging());

        h.start_drag(DropData::Text("dragged text".into()));
        assert!(h.is_dragging());
        assert!(h.can_drop((100.0, 100.0)));

        let data = h.accept_drop((100.0, 100.0));
        assert_eq!(data.as_text(), Some("dragged text"));
        assert!(!h.is_dragging());
    }

    #[test]
    fn headless_cancel_drag() {
        let mut h = HeadlessDrop::new();
        h.start_drag(DropData::Index(5));
        assert!(h.is_dragging());

        h.cancel_drag();
        assert!(!h.is_dragging());
        assert!(h.get_drag_data().is_none());
    }

    #[test]
    fn headless_cannot_drop_when_not_dragging() {
        let h = HeadlessDrop::new();
        assert!(!h.can_drop((0.0, 0.0)));
    }

    #[test]
    fn headless_accept_drop_when_not_dragging_returns_none() {
        let mut h = HeadlessDrop::new();
        let data = h.accept_drop((0.0, 0.0));
        assert!(data.is_none());
    }

    // -- Drop zone ----------------------------------------------------------

    #[test]
    fn headless_drop_zone_rejects_outside() {
        let mut h = HeadlessDrop::new();
        h.set_drop_zone(100.0, 100.0, 200.0, 200.0);
        h.start_drag(DropData::Text("hello".into()));

        assert!(!h.can_drop((50.0, 50.0))); // outside
        assert!(h.can_drop((150.0, 150.0))); // inside
        assert!(h.can_drop((100.0, 100.0))); // edge
        assert!(h.can_drop((300.0, 300.0))); // edge
        assert!(!h.can_drop((301.0, 150.0))); // just outside
    }

    #[test]
    fn headless_clear_drop_zone() {
        let mut h = HeadlessDrop::new();
        h.set_drop_zone(100.0, 100.0, 10.0, 10.0);
        h.start_drag(DropData::Text("hello".into()));
        assert!(!h.can_drop((0.0, 0.0)));

        h.clear_drop_zone();
        assert!(h.can_drop((0.0, 0.0)));
    }

    // -- Filter integration -------------------------------------------------

    #[test]
    fn headless_with_filter_rejects_wrong_extension() {
        let mut h = HeadlessDrop::with_filter(DropFilter::extensions(&["tscn"]));
        h.start_drag(DropData::Files(vec!["script.gd".into()]));
        assert!(!h.can_drop((0.0, 0.0)));
    }

    #[test]
    fn headless_with_filter_accepts_correct_extension() {
        let mut h = HeadlessDrop::with_filter(DropFilter::extensions(&["tscn"]));
        h.start_drag(DropData::Files(vec!["scene.tscn".into()]));
        assert!(h.can_drop((0.0, 0.0)));
    }

    #[test]
    fn headless_filter_does_not_affect_non_file_data() {
        let mut h = HeadlessDrop::with_filter(DropFilter::extensions(&["tscn"]));
        h.start_drag(DropData::Text("hello".into()));
        // Text data is not filtered by file extension filter.
        assert!(h.can_drop((0.0, 0.0)));
    }

    // -- Full lifecycle with OS + internal ----------------------------------

    #[test]
    fn full_drag_drop_workflow() {
        let mut h = HeadlessDrop::new();

        // OS file drop
        h.drop_context_mut().drag_enter();
        h.drop_context_mut().drop_files(vec!["level.tscn".into()]);
        let os_files = h.drop_context_mut().take_pending_files();
        assert_eq!(os_files, vec!["level.tscn"]);

        // Internal drag
        h.start_drag(DropData::Files(vec!["res://icon.png".into()]));
        assert!(h.is_dragging());
        let data = h.accept_drop((50.0, 50.0));
        assert_eq!(data.as_files().unwrap(), &["res://icon.png"]);
        assert!(!h.is_dragging());
    }
}
