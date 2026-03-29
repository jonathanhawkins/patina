//! Editor project manager for creating and opening projects.
//!
//! Implements Godot's project manager dialog functionality:
//!
//! - **Create projects**: initialize a new Godot project directory with
//!   `project.godot`, default environment, and folder structure.
//! - **Open projects**: scan and validate existing project directories.
//! - **Project list**: track known projects with metadata (name, path,
//!   last opened, Godot version, icon).
//! - **Import**: import existing projects by locating `project.godot`.
//! - **Remove**: remove projects from the list (without deleting files).
//! - **Sort/filter**: sort by name, last opened, or path; filter by search.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

/// Godot rendering backend selection for new projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderingBackend {
    /// Forward+ (high-end 3D).
    ForwardPlus,
    /// Mobile-optimized renderer.
    Mobile,
    /// Compatibility / GL (widest support).
    Compatibility,
}

impl RenderingBackend {
    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ForwardPlus => "Forward+",
            Self::Mobile => "Mobile",
            Self::Compatibility => "Compatibility",
        }
    }

    /// Config value written to `project.godot`.
    pub fn config_value(&self) -> &'static str {
        match self {
            Self::ForwardPlus => "forward_plus",
            Self::Mobile => "mobile",
            Self::Compatibility => "gl_compatibility",
        }
    }
}

impl Default for RenderingBackend {
    fn default() -> Self {
        Self::ForwardPlus
    }
}

// ---------------------------------------------------------------------------
// VersionControl
// ---------------------------------------------------------------------------

/// Version control system to initialize with the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionControl {
    /// No VCS initialization.
    None,
    /// Initialize a Git repository.
    Git,
}

impl Default for VersionControl {
    fn default() -> Self {
        Self::None
    }
}

// ---------------------------------------------------------------------------
// ProjectInfo
// ---------------------------------------------------------------------------

/// Metadata about a known project.
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    /// Display name of the project.
    pub name: String,
    /// Absolute path to the project directory.
    pub path: PathBuf,
    /// When this project was last opened.
    pub last_opened: SystemTime,
    /// Godot version string (e.g. "4.3").
    pub godot_version: String,
    /// Rendering backend.
    pub renderer: RenderingBackend,
    /// Optional project icon path (relative to project dir).
    pub icon: Option<String>,
    /// Optional description from project.godot.
    pub description: String,
    /// Whether the project has been marked as favorite.
    pub favorite: bool,
    /// Feature tags declared in project.godot.
    pub features: Vec<String>,
}

impl ProjectInfo {
    /// Create a new ProjectInfo with the given name and path.
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            last_opened: SystemTime::now(),
            godot_version: String::from("4.3"),
            renderer: RenderingBackend::default(),
            icon: None,
            description: String::new(),
            favorite: false,
            features: Vec::new(),
        }
    }

    /// Touch the last-opened timestamp to now.
    pub fn touch(&mut self) {
        self.last_opened = SystemTime::now();
    }

    /// Check if the project directory looks valid (has project.godot).
    pub fn is_valid(&self) -> bool {
        self.path.join("project.godot").exists()
    }

    /// Returns the project.godot path.
    pub fn project_file(&self) -> PathBuf {
        self.path.join("project.godot")
    }
}

impl PartialEq for ProjectInfo {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

// ---------------------------------------------------------------------------
// ProjectCreateOptions
// ---------------------------------------------------------------------------

/// Options for creating a new Godot project.
#[derive(Debug, Clone)]
pub struct ProjectCreateOptions {
    /// Project name.
    pub name: String,
    /// Parent directory where the project folder will be created.
    pub parent_dir: PathBuf,
    /// Rendering backend to use.
    pub renderer: RenderingBackend,
    /// Version control to initialize.
    pub vcs: VersionControl,
    /// Initial feature tags.
    pub features: Vec<String>,
}

impl ProjectCreateOptions {
    /// Create new options with the given name and parent directory.
    pub fn new(name: impl Into<String>, parent_dir: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            parent_dir: parent_dir.into(),
            renderer: RenderingBackend::default(),
            vcs: VersionControl::default(),
            features: Vec::new(),
        }
    }

    /// The full project directory path: `parent_dir / name`.
    pub fn project_dir(&self) -> PathBuf {
        self.parent_dir.join(&self.name)
    }

    /// Generate the `project.godot` file content.
    pub fn generate_project_godot(&self) -> String {
        let mut content = String::new();
        content.push_str("; Engine configuration file.\n");
        content.push_str("; It's best edited using the editor UI and not directly,\n");
        content.push_str("; since the parameters that go here are not all obvious.\n");
        content.push_str(";\n");
        content.push_str("; Format:\n");
        content.push_str(";   [section] key=value\n\n");

        content.push_str("[application]\n\n");
        content.push_str(&format!(
            "config/name=\"{}\"\n",
            self.name.replace('"', "\\\"")
        ));
        content.push_str("config/features=PackedStringArray(");
        let mut feat_parts: Vec<String> = vec![format!("\"4.3\"")];
        if self.renderer != RenderingBackend::ForwardPlus {
            feat_parts.push(format!("\"{}\"", self.renderer.label()));
        }
        for f in &self.features {
            feat_parts.push(format!("\"{}\"", f.replace('"', "\\\"")));
        }
        content.push_str(&feat_parts.join(", "));
        content.push_str(")\n");
        content.push_str("config/icon=\"res://icon.svg\"\n");

        if self.renderer != RenderingBackend::ForwardPlus {
            content.push_str("\n[rendering]\n\n");
            content.push_str(&format!(
                "renderer/rendering_method=\"{}\"\n",
                self.renderer.config_value()
            ));
        }

        content
    }

    /// Generate a default .gitignore for Godot projects.
    pub fn generate_gitignore(&self) -> &'static str {
        "# Godot 4 .gitignore\n\
         .godot/\n\
         *.import\n\
         export_presets.cfg\n\
         # Mono\n\
         .mono/\n\
         data_*/\n\
         mono_crash.*.json\n"
    }

    /// Generate a minimal default icon SVG.
    pub fn generate_default_icon(&self) -> &'static str {
        r##"<svg height="128" width="128" xmlns="http://www.w3.org/2000/svg"><rect x="2" y="2" width="124" height="124" rx="14" fill="#363d52" stroke="#212532" stroke-width="4"/><g transform="translate(32,32)" fill="#fff"><circle cx="32" cy="24" r="8"/><rect x="16" y="48" width="32" height="8" rx="4"/></g></svg>"##
    }
}

// ---------------------------------------------------------------------------
// SortField
// ---------------------------------------------------------------------------

/// How to sort the project list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    /// Sort by project name (alphabetical).
    Name,
    /// Sort by last-opened timestamp (most recent first).
    LastOpened,
    /// Sort by project path (alphabetical).
    Path,
}

impl Default for SortField {
    fn default() -> Self {
        Self::LastOpened
    }
}

// ---------------------------------------------------------------------------
// ProjectManager
// ---------------------------------------------------------------------------

/// The project manager: tracks known projects and provides create/open/import.
#[derive(Debug)]
pub struct ProjectManager {
    /// Known projects, keyed by canonical path string.
    projects: HashMap<String, ProjectInfo>,
    /// Current sort field.
    pub sort_field: SortField,
    /// Whether sort is ascending (false = descending).
    pub sort_ascending: bool,
    /// Search filter (case-insensitive).
    pub search_query: String,
}

impl ProjectManager {
    /// Create a new empty project manager.
    pub fn new() -> Self {
        Self {
            projects: HashMap::new(),
            sort_field: SortField::default(),
            sort_ascending: true,
            search_query: String::new(),
        }
    }

    /// Number of known projects.
    pub fn project_count(&self) -> usize {
        self.projects.len()
    }

    /// Add or update a project in the manager.
    pub fn add_project(&mut self, info: ProjectInfo) {
        let key = path_key(&info.path);
        self.projects.insert(key, info);
    }

    /// Remove a project from the manager by path. Does NOT delete files.
    /// Returns the removed project info if it existed.
    pub fn remove_project(&mut self, path: &Path) -> Option<ProjectInfo> {
        let key = path_key(path);
        self.projects.remove(&key)
    }

    /// Get a project by path.
    pub fn get_project(&self, path: &Path) -> Option<&ProjectInfo> {
        let key = path_key(path);
        self.projects.get(&key)
    }

    /// Get a mutable project by path.
    pub fn get_project_mut(&mut self, path: &Path) -> Option<&mut ProjectInfo> {
        let key = path_key(path);
        self.projects.get_mut(&key)
    }

    /// Open a project: touch its timestamp and return its info.
    pub fn open_project(&mut self, path: &Path) -> Option<&ProjectInfo> {
        let key = path_key(path);
        if let Some(info) = self.projects.get_mut(&key) {
            info.touch();
            Some(info)
        } else {
            None
        }
    }

    /// Toggle favorite status for a project.
    pub fn toggle_favorite(&mut self, path: &Path) -> Option<bool> {
        let key = path_key(path);
        if let Some(info) = self.projects.get_mut(&key) {
            info.favorite = !info.favorite;
            Some(info.favorite)
        } else {
            None
        }
    }

    /// Import a project from a path containing `project.godot`.
    /// Returns an error string if the path is invalid.
    pub fn import_project(&mut self, project_dir: &Path) -> Result<&ProjectInfo, String> {
        let godot_file = project_dir.join("project.godot");
        if !godot_file.exists() {
            return Err(format!(
                "No project.godot found in {}",
                project_dir.display()
            ));
        }

        let name = project_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unnamed".into());

        let info = ProjectInfo::new(name, project_dir);
        let key = path_key(project_dir);
        self.projects.insert(key.clone(), info);
        Ok(self.projects.get(&key).unwrap())
    }

    /// Create a new project from options. Returns the generated project.godot
    /// content and the project info. Does NOT write to disk — the caller
    /// is responsible for filesystem operations.
    pub fn create_project(
        &mut self,
        options: &ProjectCreateOptions,
    ) -> Result<(ProjectInfo, ProjectContents), String> {
        let project_dir = options.project_dir();

        if self.projects.contains_key(&path_key(&project_dir)) {
            return Err(format!(
                "Project already exists at {}",
                project_dir.display()
            ));
        }

        let mut info = ProjectInfo::new(&options.name, &project_dir);
        info.renderer = options.renderer;
        info.features = options.features.clone();

        let contents = ProjectContents {
            project_godot: options.generate_project_godot(),
            gitignore: if options.vcs == VersionControl::Git {
                Some(options.generate_gitignore().to_string())
            } else {
                None
            },
            default_icon_svg: options.generate_default_icon().to_string(),
            directories: vec![
                "res://".to_string(),
                "scenes/".to_string(),
                "scripts/".to_string(),
                "assets/".to_string(),
            ],
        };

        let key = path_key(&project_dir);
        self.projects.insert(key, info.clone());

        Ok((info, contents))
    }

    /// Return all projects, sorted and filtered according to current settings.
    pub fn list_projects(&self) -> Vec<&ProjectInfo> {
        let mut projects: Vec<&ProjectInfo> = self.projects.values().collect();

        // Filter by search query
        if !self.search_query.is_empty() {
            let query = self.search_query.to_lowercase();
            projects.retain(|p| {
                p.name.to_lowercase().contains(&query)
                    || p.path.to_string_lossy().to_lowercase().contains(&query)
                    || p.description.to_lowercase().contains(&query)
            });
        }

        // Sort: favorites always first, then by sort field
        projects.sort_by(|a, b| {
            // Favorites first
            let fav_cmp = b.favorite.cmp(&a.favorite);
            if fav_cmp != std::cmp::Ordering::Equal {
                return fav_cmp;
            }

            let ord = match self.sort_field {
                SortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortField::LastOpened => a.last_opened.cmp(&b.last_opened).reverse(),
                SortField::Path => a
                    .path
                    .to_string_lossy()
                    .to_lowercase()
                    .cmp(&b.path.to_string_lossy().to_lowercase()),
            };

            if self.sort_ascending {
                ord
            } else {
                ord.reverse()
            }
        });

        projects
    }

    /// Return only favorite projects.
    pub fn favorites(&self) -> Vec<&ProjectInfo> {
        self.projects.values().filter(|p| p.favorite).collect()
    }

    /// Remove all projects that no longer have a valid project.godot on disk.
    /// Returns the number of projects removed.
    pub fn remove_missing(&mut self) -> usize {
        let missing: Vec<String> = self
            .projects
            .iter()
            .filter(|(_, info)| !info.is_valid())
            .map(|(key, _)| key.clone())
            .collect();
        let count = missing.len();
        for key in missing {
            self.projects.remove(&key);
        }
        count
    }

    /// Clear all projects from the manager.
    pub fn clear(&mut self) {
        self.projects.clear();
    }
}

impl Default for ProjectManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// ProjectContents
// ---------------------------------------------------------------------------

/// Files to write when creating a new project (returned by `create_project`).
#[derive(Debug, Clone)]
pub struct ProjectContents {
    /// Content for `project.godot`.
    pub project_godot: String,
    /// Content for `.gitignore` (only if Git VCS selected).
    pub gitignore: Option<String>,
    /// Content for `icon.svg`.
    pub default_icon_svg: String,
    /// Directories to create.
    pub directories: Vec<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Normalize a path to a string key for the HashMap.
fn path_key(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_path(name: &str) -> PathBuf {
        PathBuf::from(format!("/tmp/test_projects/{}", name))
    }

    // -- RenderingBackend --

    #[test]
    fn rendering_backend_default_is_forward_plus() {
        assert_eq!(RenderingBackend::default(), RenderingBackend::ForwardPlus);
    }

    #[test]
    fn rendering_backend_labels() {
        assert_eq!(RenderingBackend::ForwardPlus.label(), "Forward+");
        assert_eq!(RenderingBackend::Mobile.label(), "Mobile");
        assert_eq!(RenderingBackend::Compatibility.label(), "Compatibility");
    }

    #[test]
    fn rendering_backend_config_values() {
        assert_eq!(RenderingBackend::ForwardPlus.config_value(), "forward_plus");
        assert_eq!(RenderingBackend::Mobile.config_value(), "mobile");
        assert_eq!(
            RenderingBackend::Compatibility.config_value(),
            "gl_compatibility"
        );
    }

    // -- ProjectInfo --

    #[test]
    fn project_info_new() {
        let info = ProjectInfo::new("MyGame", "/tmp/MyGame");
        assert_eq!(info.name, "MyGame");
        assert_eq!(info.path, PathBuf::from("/tmp/MyGame"));
        assert_eq!(info.godot_version, "4.3");
        assert!(!info.favorite);
        assert!(info.features.is_empty());
        assert!(info.description.is_empty());
    }

    #[test]
    fn project_info_touch_updates_time() {
        let mut info = ProjectInfo::new("A", "/tmp/A");
        let t1 = info.last_opened;
        // Small spin to ensure time advances (SystemTime may have ms resolution)
        std::thread::sleep(std::time::Duration::from_millis(10));
        info.touch();
        assert!(info.last_opened >= t1);
    }

    #[test]
    fn project_info_equality_by_path() {
        let a = ProjectInfo::new("Game1", "/tmp/A");
        let mut b = ProjectInfo::new("DifferentName", "/tmp/A");
        b.favorite = true;
        assert_eq!(a, b); // same path = equal
    }

    #[test]
    fn project_info_project_file() {
        let info = ProjectInfo::new("G", "/home/user/G");
        assert_eq!(
            info.project_file(),
            PathBuf::from("/home/user/G/project.godot")
        );
    }

    // -- ProjectCreateOptions --

    #[test]
    fn create_options_project_dir() {
        let opts = ProjectCreateOptions::new("MyGame", "/home/user/projects");
        assert_eq!(
            opts.project_dir(),
            PathBuf::from("/home/user/projects/MyGame")
        );
    }

    #[test]
    fn create_options_default_renderer() {
        let opts = ProjectCreateOptions::new("G", "/tmp");
        assert_eq!(opts.renderer, RenderingBackend::ForwardPlus);
    }

    #[test]
    fn generate_project_godot_forward_plus() {
        let opts = ProjectCreateOptions::new("TestGame", "/tmp");
        let content = opts.generate_project_godot();
        assert!(content.contains("config/name=\"TestGame\""));
        assert!(content.contains("config/icon=\"res://icon.svg\""));
        // Forward+ is default, so no [rendering] section
        assert!(!content.contains("[rendering]"));
    }

    #[test]
    fn generate_project_godot_mobile() {
        let mut opts = ProjectCreateOptions::new("MobileGame", "/tmp");
        opts.renderer = RenderingBackend::Mobile;
        let content = opts.generate_project_godot();
        assert!(content.contains("[rendering]"));
        assert!(content.contains("renderer/rendering_method=\"mobile\""));
        assert!(content.contains("\"Mobile\""));
    }

    #[test]
    fn generate_project_godot_compatibility() {
        let mut opts = ProjectCreateOptions::new("WebGame", "/tmp");
        opts.renderer = RenderingBackend::Compatibility;
        let content = opts.generate_project_godot();
        assert!(content.contains("renderer/rendering_method=\"gl_compatibility\""));
    }

    #[test]
    fn generate_project_godot_custom_features() {
        let mut opts = ProjectCreateOptions::new("Feat", "/tmp");
        opts.features = vec!["3D".into(), "Physics".into()];
        let content = opts.generate_project_godot();
        assert!(content.contains("\"3D\""));
        assert!(content.contains("\"Physics\""));
    }

    #[test]
    fn generate_project_godot_escapes_quotes_in_name() {
        let opts = ProjectCreateOptions::new("My \"Game\"", "/tmp");
        let content = opts.generate_project_godot();
        assert!(content.contains("config/name=\"My \\\"Game\\\"\""));
    }

    #[test]
    fn generate_gitignore() {
        let opts = ProjectCreateOptions::new("G", "/tmp");
        let gi = opts.generate_gitignore();
        assert!(gi.contains(".godot/"));
        assert!(gi.contains("*.import"));
    }

    #[test]
    fn generate_default_icon_is_svg() {
        let opts = ProjectCreateOptions::new("G", "/tmp");
        let icon = opts.generate_default_icon();
        assert!(icon.starts_with("<svg"));
        assert!(icon.contains("</svg>"));
    }

    // -- ProjectManager --

    #[test]
    fn manager_new_is_empty() {
        let pm = ProjectManager::new();
        assert_eq!(pm.project_count(), 0);
        assert!(pm.list_projects().is_empty());
    }

    #[test]
    fn manager_add_and_get() {
        let mut pm = ProjectManager::new();
        let info = ProjectInfo::new("Game1", test_path("game1"));
        pm.add_project(info);
        assert_eq!(pm.project_count(), 1);
        let got = pm.get_project(&test_path("game1")).unwrap();
        assert_eq!(got.name, "Game1");
    }

    #[test]
    fn manager_add_duplicate_overwrites() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("V1", test_path("game")));
        pm.add_project(ProjectInfo::new("V2", test_path("game")));
        assert_eq!(pm.project_count(), 1);
        assert_eq!(pm.get_project(&test_path("game")).unwrap().name, "V2");
    }

    #[test]
    fn manager_remove() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("G", test_path("g")));
        let removed = pm.remove_project(&test_path("g"));
        assert!(removed.is_some());
        assert_eq!(pm.project_count(), 0);
    }

    #[test]
    fn manager_remove_nonexistent() {
        let mut pm = ProjectManager::new();
        assert!(pm.remove_project(&test_path("nope")).is_none());
    }

    #[test]
    fn manager_open_project_touches() {
        let mut pm = ProjectManager::new();
        let info = ProjectInfo::new("G", test_path("g"));
        let t0 = info.last_opened;
        pm.add_project(info);
        std::thread::sleep(std::time::Duration::from_millis(10));
        let opened = pm.open_project(&test_path("g")).unwrap();
        assert!(opened.last_opened >= t0);
    }

    #[test]
    fn manager_open_nonexistent() {
        let mut pm = ProjectManager::new();
        assert!(pm.open_project(&test_path("nope")).is_none());
    }

    #[test]
    fn manager_toggle_favorite() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("G", test_path("g")));
        assert_eq!(pm.toggle_favorite(&test_path("g")), Some(true));
        assert_eq!(pm.toggle_favorite(&test_path("g")), Some(false));
        assert_eq!(pm.toggle_favorite(&test_path("nope")), None);
    }

    #[test]
    fn manager_favorites_filter() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("A", test_path("a")));
        pm.add_project(ProjectInfo::new("B", test_path("b")));
        pm.toggle_favorite(&test_path("b"));
        let favs = pm.favorites();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].name, "B");
    }

    #[test]
    fn manager_search_by_name() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("Platformer", test_path("plat")));
        pm.add_project(ProjectInfo::new("RPG", test_path("rpg")));
        pm.search_query = "plat".into();
        let results = pm.list_projects();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Platformer");
    }

    #[test]
    fn manager_search_by_path() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("A", test_path("alpha")));
        pm.add_project(ProjectInfo::new("B", test_path("beta")));
        pm.search_query = "alpha".into();
        let results = pm.list_projects();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "A");
    }

    #[test]
    fn manager_search_case_insensitive() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("MyGame", test_path("mg")));
        pm.search_query = "MYGAME".into();
        assert_eq!(pm.list_projects().len(), 1);
    }

    #[test]
    fn manager_search_empty_returns_all() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("A", test_path("a")));
        pm.add_project(ProjectInfo::new("B", test_path("b")));
        pm.search_query.clear();
        assert_eq!(pm.list_projects().len(), 2);
    }

    #[test]
    fn manager_sort_by_name_ascending() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("Zebra", test_path("z")));
        pm.add_project(ProjectInfo::new("Alpha", test_path("a")));
        pm.sort_field = SortField::Name;
        pm.sort_ascending = true;
        let list = pm.list_projects();
        assert_eq!(list[0].name, "Alpha");
        assert_eq!(list[1].name, "Zebra");
    }

    #[test]
    fn manager_sort_by_name_descending() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("Alpha", test_path("a")));
        pm.add_project(ProjectInfo::new("Zebra", test_path("z")));
        pm.sort_field = SortField::Name;
        pm.sort_ascending = false;
        let list = pm.list_projects();
        assert_eq!(list[0].name, "Zebra");
        assert_eq!(list[1].name, "Alpha");
    }

    #[test]
    fn manager_sort_by_path() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("B", test_path("zzz")));
        pm.add_project(ProjectInfo::new("A", test_path("aaa")));
        pm.sort_field = SortField::Path;
        pm.sort_ascending = true;
        let list = pm.list_projects();
        assert!(list[0].path.to_string_lossy().contains("aaa"));
    }

    #[test]
    fn manager_favorites_sort_first() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("Alpha", test_path("a")));
        pm.add_project(ProjectInfo::new("Zebra", test_path("z")));
        pm.toggle_favorite(&test_path("z"));
        pm.sort_field = SortField::Name;
        pm.sort_ascending = true;
        let list = pm.list_projects();
        // Zebra is favorite, should come first despite name sort
        assert_eq!(list[0].name, "Zebra");
        assert_eq!(list[1].name, "Alpha");
    }

    #[test]
    fn manager_create_project() {
        let mut pm = ProjectManager::new();
        let opts = ProjectCreateOptions::new("NewGame", "/tmp/projects");
        let (info, contents) = pm.create_project(&opts).unwrap();
        assert_eq!(info.name, "NewGame");
        assert!(contents.project_godot.contains("config/name=\"NewGame\""));
        assert!(contents.gitignore.is_none()); // no VCS by default
        assert!(contents.default_icon_svg.contains("<svg"));
        assert_eq!(contents.directories.len(), 4);
        // Project should be registered
        assert_eq!(pm.project_count(), 1);
    }

    #[test]
    fn manager_create_project_with_git() {
        let mut pm = ProjectManager::new();
        let mut opts = ProjectCreateOptions::new("GitGame", "/tmp/projects");
        opts.vcs = VersionControl::Git;
        let (_, contents) = pm.create_project(&opts).unwrap();
        assert!(contents.gitignore.is_some());
        assert!(contents.gitignore.unwrap().contains(".godot/"));
    }

    #[test]
    fn manager_create_duplicate_errors() {
        let mut pm = ProjectManager::new();
        let opts = ProjectCreateOptions::new("Dup", "/tmp/projects");
        pm.create_project(&opts).unwrap();
        let result = pm.create_project(&opts);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn manager_create_with_mobile_renderer() {
        let mut pm = ProjectManager::new();
        let mut opts = ProjectCreateOptions::new("MobileG", "/tmp/p");
        opts.renderer = RenderingBackend::Mobile;
        let (info, contents) = pm.create_project(&opts).unwrap();
        assert_eq!(info.renderer, RenderingBackend::Mobile);
        assert!(contents.project_godot.contains("\"mobile\""));
    }

    #[test]
    fn manager_clear() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("A", test_path("a")));
        pm.add_project(ProjectInfo::new("B", test_path("b")));
        pm.clear();
        assert_eq!(pm.project_count(), 0);
    }

    #[test]
    fn manager_get_project_mut() {
        let mut pm = ProjectManager::new();
        pm.add_project(ProjectInfo::new("G", test_path("g")));
        let info = pm.get_project_mut(&test_path("g")).unwrap();
        info.description = "Updated".into();
        assert_eq!(
            pm.get_project(&test_path("g")).unwrap().description,
            "Updated"
        );
    }

    #[test]
    fn version_control_default_is_none() {
        assert_eq!(VersionControl::default(), VersionControl::None);
    }

    #[test]
    fn sort_field_default_is_last_opened() {
        assert_eq!(SortField::default(), SortField::LastOpened);
    }
}
