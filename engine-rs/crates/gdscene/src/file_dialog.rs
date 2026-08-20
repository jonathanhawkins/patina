//! `FileDialog` listing, filtering and selection (pat-s3rrv.162).
//!
//! Godot's `FileDialog` browses a directory subject to four settings that
//! interact: `access` picks which root the paths are expressed against,
//! `root_subfolder` confines browsing beneath it, `filters` hides files whose
//! name matches no pattern, and `show_hidden_files` decides whether dotfiles
//! appear at all.
//!
//! This is the pure half — directory in, entries out — so it is testable
//! without a dialog, a theme or a frame.

use std::path::{Path, PathBuf};

/// Godot `FileDialog.Access`. Values are the enum's, not invented:
/// `doc/classes/FileDialog.xml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Access {
    /// `res://` — the project tree. Godot's default (value 0).
    #[default]
    Resources,
    /// `user://` — the writable per-project data directory.
    UserData,
    /// Absolute paths anywhere on disk.
    Filesystem,
}

impl Access {
    /// From the integer a `.tscn` or a script stores.
    pub fn from_int(v: i64) -> Self {
        match v {
            1 => Access::UserData,
            2 => Access::Filesystem,
            // ⚠️ 0 AND anything unrecognised map to Resources, because that is
            // Godot's default and a dialog that silently browsed the whole
            // filesystem on a bad value would be a sandbox escape, not a
            // cosmetic bug.
            _ => Access::Resources,
        }
    }

    /// The scheme prefix a selection is reported with, if any.
    pub fn scheme(self) -> Option<&'static str> {
        match self {
            Access::Resources => Some("res://"),
            Access::UserData => Some("user://"),
            Access::Filesystem => None,
        }
    }
}

/// The dialog's browsing settings.
#[derive(Debug, Clone, Default)]
pub struct FileDialogState {
    pub access: Access,
    /// Confines browsing to this subfolder of the access root. Empty means the
    /// root itself.
    pub root_subfolder: String,
    /// Godot filter strings: `"*.png, *.jpg ; Supported Images"`.
    pub filters: Vec<String>,
    pub show_hidden_files: bool,
}

/// One row in the dialog's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
}

/// The extension patterns a Godot filter string carries.
///
/// The format is `"<patterns> ; <description>"`, patterns comma-separated. The
/// description is for humans and must not be matched against — a filter of
/// `"*.png ; PNG Images"` would otherwise match a file called `Images`.
fn filter_patterns(filter: &str) -> Vec<String> {
    let patterns = filter.split(';').next().unwrap_or("");
    patterns
        .split(',')
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty())
        .collect()
}

/// Whether `name` passes any of `filters`.
///
/// An EMPTY filter list shows everything, which is Godot's behaviour and the
/// default — treating "no filters" as "match nothing" would present an empty
/// dialog for the commonest configuration.
pub fn matches_filters(name: &str, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    filters.iter().flat_map(|f| filter_patterns(f)).any(|pat| {
        match pat.strip_prefix('*') {
            // `*.png` — the usual form: a suffix match.
            Some(suffix) => lower.ends_with(suffix),
            // A bare name in a filter is an exact match, not a substring one.
            None => lower == pat,
        }
    })
}

/// Whether a name is hidden in Godot's sense: a leading dot.
pub fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// The directory the dialog actually browses, honouring `root_subfolder`.
///
/// ⚠️ `root_subfolder` is a CONFINEMENT, so a traversal segment in it would
/// escape the very boundary it exists to enforce. Such a subfolder is ignored
/// and the root is used instead.
pub fn browse_root(access_root: &Path, state: &FileDialogState) -> PathBuf {
    let sub = state.root_subfolder.trim_matches('/');
    if sub.is_empty() || sub.split(['/', '\\']).any(|seg| seg == "..") {
        return access_root.to_path_buf();
    }
    access_root.join(sub)
}

/// Lists a directory as the dialog would show it.
///
/// Directories are listed regardless of `filters` — a filter selects files to
/// open, and hiding folders would make their contents unreachable. They sort
/// first, then files, each alphabetically, so the order does not depend on the
/// filesystem's.
pub fn list_entries(dir: &Path, state: &FileDialogState) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = read
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !state.show_hidden_files && is_hidden(&name) {
                return None;
            }
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if !is_dir && !matches_filters(&name, &state.filters) {
                return None;
            }
            Some(Entry { name, is_dir })
        })
        .collect();
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    entries
}

/// The path `file_selected` reports for a chosen entry.
///
/// Under `ACCESS_RESOURCES` this is a `res://` path including the
/// `root_subfolder`, which is what a script receiving the signal can actually
/// `load()`. An absolute filesystem path would be unusable there.
pub fn selected_path(state: &FileDialogState, name: &str) -> String {
    let sub = state.root_subfolder.trim_matches('/');
    let relative = if sub.is_empty() {
        name.to_string()
    } else {
        format!("{sub}/{name}")
    };
    match state.access.scheme() {
        Some(scheme) => format!("{scheme}{relative}"),
        None => relative,
    }
}

/// Reads a `FileDialog` node's browsing settings off its properties.
///
/// Every field falls back to Godot's documented default, so a dialog that sets
/// only `filters` still browses `res://` with hidden files off rather than
/// picking up whatever a partially-configured node happens to carry.
pub fn state_from_node(
    tree: &crate::scene_tree::SceneTree,
    node_id: crate::node::NodeId,
) -> FileDialogState {
    let Some(node) = tree.get_node(node_id) else {
        return FileDialogState::default();
    };
    let filters = match node.get_property("filters") {
        gdvariant::Variant::PackedStringArray(v) => v,
        // A single filter is often authored as a plain string.
        gdvariant::Variant::String(s) => vec![s],
        _ => Vec::new(),
    };
    FileDialogState {
        access: match node.get_property("access") {
            gdvariant::Variant::Int(i) => Access::from_int(i),
            _ => Access::default(),
        },
        root_subfolder: match node.get_property("root_subfolder") {
            gdvariant::Variant::String(s) => s,
            _ => String::new(),
        },
        filters,
        show_hidden_files: matches!(
            node.get_property("show_hidden_files"),
            gdvariant::Variant::Bool(true)
        ),
    }
}

/// Confirms a selection: emits `file_selected` with the path a script can
/// actually load, and returns that path.
///
/// ⚠️ The signal carries the **access-qualified** path (`res://art/icon.png`),
/// not the bare entry name the list displayed. A receiver's whole purpose is to
/// `load()` what was picked, and a bare name is not loadable.
pub fn confirm_selection(
    tree: &mut crate::scene_tree::SceneTree,
    node_id: crate::node::NodeId,
    entry_name: &str,
) -> String {
    let state = state_from_node(tree, node_id);
    let path = selected_path(&state, entry_name);
    tree.emit_signal(
        node_id,
        "file_selected",
        &[gdvariant::Variant::String(path.clone())],
    );
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(filters: &[&str], hidden: bool) -> FileDialogState {
        FileDialogState {
            filters: filters.iter().map(|s| (*s).to_string()).collect(),
            show_hidden_files: hidden,
            ..Default::default()
        }
    }

    #[test]
    fn a_filters_description_is_not_matched_against() {
        let f = state(&["*.png ; PNG Images"], false);
        assert!(matches_filters("logo.png", &f.filters));
        // The description says "Images"; a file of that name must not pass.
        assert!(!matches_filters("Images", &f.filters));
        assert!(!matches_filters("notes.txt", &f.filters));
    }

    #[test]
    fn several_patterns_in_one_filter_all_apply() {
        let f = state(&["*.png, *.jpg ; Supported Images"], false);
        assert!(matches_filters("a.png", &f.filters));
        assert!(
            matches_filters("b.JPG", &f.filters),
            "matching is case-insensitive"
        );
        assert!(!matches_filters("c.gif", &f.filters));
    }

    /// No filters shows everything — the default configuration.
    #[test]
    fn an_empty_filter_list_matches_everything() {
        assert!(matches_filters("anything.xyz", &[]));
    }

    #[test]
    fn access_falls_back_to_resources_on_an_unknown_value() {
        assert_eq!(Access::from_int(0), Access::Resources);
        assert_eq!(Access::from_int(1), Access::UserData);
        assert_eq!(Access::from_int(2), Access::Filesystem);
        // Not Filesystem: a bad value must not widen access.
        assert_eq!(Access::from_int(99), Access::Resources);
        assert_eq!(Access::from_int(-1), Access::Resources);
    }

    #[test]
    fn a_selection_is_reported_as_a_loadable_res_path() {
        let mut s = state(&[], false);
        assert_eq!(selected_path(&s, "icon.png"), "res://icon.png");
        s.root_subfolder = "art".to_string();
        assert_eq!(selected_path(&s, "icon.png"), "res://art/icon.png");
        s.access = Access::UserData;
        assert_eq!(selected_path(&s, "save.dat"), "user://art/save.dat");
        s.access = Access::Filesystem;
        assert_eq!(selected_path(&s, "save.dat"), "art/save.dat");
    }

    /// ⚠️ `root_subfolder` confines browsing, so a traversal in it must not
    /// escape the root it is confining to.
    #[test]
    fn a_traversing_root_subfolder_is_ignored() {
        let root = Path::new("/tmp/project");
        let mut s = state(&[], false);
        s.root_subfolder = "../etc".to_string();
        assert_eq!(browse_root(root, &s), root.to_path_buf());
        s.root_subfolder = "art/../../etc".to_string();
        assert_eq!(browse_root(root, &s), root.to_path_buf());
        s.root_subfolder = "art".to_string();
        assert_eq!(browse_root(root, &s), root.join("art"));
    }
}
