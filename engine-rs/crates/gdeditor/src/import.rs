//! Resource import pipeline.
//!
//! Provides the [`ResourceImporter`] trait for pluggable import backends,
//! an [`ImportPipeline`] that orchestrates scanning and importing, and
//! built-in importers for `.tres` and `.tscn` files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gdcore::error::{EngineError, EngineResult};
use gdvariant::Variant;

/// The result of a successful import.
#[derive(Debug, Clone)]
pub struct ImportedResource {
    /// The source path that was imported.
    pub source_path: PathBuf,
    /// The resource type name (e.g. `"PackedScene"`, `"Texture2D"`).
    pub resource_type: String,
}

/// Trait for pluggable resource importers.
///
/// Each importer declares which file extensions it handles and provides
/// an `import` method that produces an [`ImportedResource`].
pub trait ResourceImporter: std::fmt::Debug {
    /// Returns `true` if this importer can handle the given file extension.
    fn can_import(&self, extension: &str) -> bool;

    /// Attempts to import the file at `path`.
    fn import(&self, path: &Path) -> EngineResult<ImportedResource>;

    /// Returns a human-readable name for this importer.
    fn name(&self) -> &str;
}

/// Tracks the import state of a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportState {
    /// Not yet imported.
    Pending,
    /// Successfully imported with this content hash.
    Imported { hash: u64 },
    /// Import failed with an error message.
    Failed { reason: String },
}

/// A simple hash function for content-based change detection.
fn hash_bytes(data: &[u8]) -> u64 {
    // FNV-1a 64-bit
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Orchestrates resource importing across registered importers.
///
/// Maintains a registry of [`ResourceImporter`] backends and tracks
/// per-file import state with hash-based change detection.
#[derive(Debug)]
pub struct ImportPipeline {
    /// Registered importers, checked in order.
    importers: Vec<Box<dyn ResourceImporter>>,
    /// Per-path import state.
    state: HashMap<PathBuf, ImportState>,
}

impl ImportPipeline {
    /// Creates a new empty pipeline.
    pub fn new() -> Self {
        Self {
            importers: Vec::new(),
            state: HashMap::new(),
        }
    }

    /// Registers an importer backend.
    pub fn register(&mut self, importer: Box<dyn ResourceImporter>) {
        tracing::debug!("Registered importer: {}", importer.name());
        self.importers.push(importer);
    }

    /// Returns the number of registered importers.
    pub fn importer_count(&self) -> usize {
        self.importers.len()
    }

    /// Returns the import state for a given path.
    pub fn get_state(&self, path: &Path) -> Option<&ImportState> {
        self.state.get(path)
    }

    /// Returns all tracked paths and their states.
    pub fn all_states(&self) -> &HashMap<PathBuf, ImportState> {
        &self.state
    }

    /// Scans a directory for importable files (non-recursive).
    ///
    /// Returns the list of paths that at least one importer can handle.
    pub fn scan_directory(&self, dir: &Path) -> EngineResult<Vec<PathBuf>> {
        let mut results = Vec::new();
        let entries = std::fs::read_dir(dir).map_err(EngineError::Io)?;
        for entry in entries {
            let entry = entry.map_err(EngineError::Io)?;
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if self.importers.iter().any(|imp| imp.can_import(ext)) {
                        results.push(path);
                    }
                }
            }
        }
        results.sort();
        Ok(results)
    }

    /// Imports a single file, updating state with hash-based change detection.
    ///
    /// If the file's content hash matches the previously recorded hash, the
    /// import is skipped and `Ok(None)` is returned.
    pub fn import_file(&mut self, path: &Path) -> EngineResult<Option<ImportedResource>> {
        // Read content for hashing.
        let content = std::fs::read(path).map_err(EngineError::Io)?;
        let new_hash = hash_bytes(&content);

        // Skip if unchanged.
        if let Some(ImportState::Imported { hash }) = self.state.get(path) {
            if *hash == new_hash {
                return Ok(None);
            }
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        for importer in &self.importers {
            if importer.can_import(ext) {
                match importer.import(path) {
                    Ok(resource) => {
                        self.state
                            .insert(path.to_path_buf(), ImportState::Imported { hash: new_hash });
                        return Ok(Some(resource));
                    }
                    Err(e) => {
                        self.state.insert(
                            path.to_path_buf(),
                            ImportState::Failed {
                                reason: e.to_string(),
                            },
                        );
                        return Err(e);
                    }
                }
            }
        }

        Err(EngineError::NotFound(format!(
            "no importer for extension '{ext}'"
        )))
    }

    /// Marks a path as pending (useful for forcing a re-import).
    pub fn mark_pending(&mut self, path: &Path) {
        self.state.insert(path.to_path_buf(), ImportState::Pending);
    }
}

impl Default for ImportPipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// Importer for `.tres` (text resource) files.
#[derive(Debug)]
pub struct TresImporter;

impl ResourceImporter for TresImporter {
    fn can_import(&self, extension: &str) -> bool {
        extension.eq_ignore_ascii_case("tres")
    }

    fn import(&self, path: &Path) -> EngineResult<ImportedResource> {
        // Validate the file exists and is readable.
        if !path.exists() {
            return Err(EngineError::NotFound(format!(
                "file not found: {}",
                path.display()
            )));
        }
        Ok(ImportedResource {
            source_path: path.to_path_buf(),
            resource_type: "Resource".to_string(),
        })
    }

    fn name(&self) -> &str {
        "TresImporter"
    }
}

/// Importer for `.tscn` (text scene) files.
#[derive(Debug)]
pub struct TscnImporter;

impl ResourceImporter for TscnImporter {
    fn can_import(&self, extension: &str) -> bool {
        extension.eq_ignore_ascii_case("tscn")
    }

    fn import(&self, path: &Path) -> EngineResult<ImportedResource> {
        if !path.exists() {
            return Err(EngineError::NotFound(format!(
                "file not found: {}",
                path.display()
            )));
        }
        Ok(ImportedResource {
            source_path: path.to_path_buf(),
            resource_type: "PackedScene".to_string(),
        })
    }

    fn name(&self) -> &str {
        "TscnImporter"
    }
}

// ---------------------------------------------------------------------------
// EditorSceneFormatImporter — custom scene import plugin system
// ---------------------------------------------------------------------------

/// Options controlling how a scene is imported.
#[derive(Debug, Clone)]
pub struct SceneImportOptions {
    /// Key-value import settings (e.g. "animation/import" → true).
    pub settings: HashMap<String, Variant>,
}

impl SceneImportOptions {
    pub fn new() -> Self {
        Self {
            settings: HashMap::new(),
        }
    }

    pub fn set(&mut self, key: impl Into<String>, value: Variant) {
        self.settings.insert(key.into(), value);
    }

    pub fn get(&self, key: &str) -> Option<&Variant> {
        self.settings.get(key)
    }
}

impl Default for SceneImportOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// A node in an imported scene hierarchy.
#[derive(Debug, Clone)]
pub struct ImportedSceneNode {
    /// Node name (e.g. "MeshInstance3D", "Skeleton3D").
    pub name: String,
    /// Node type class name.
    pub node_type: String,
    /// Properties set on this node.
    pub properties: HashMap<String, Variant>,
    /// Children in order.
    pub children: Vec<ImportedSceneNode>,
}

impl ImportedSceneNode {
    pub fn new(name: impl Into<String>, node_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            node_type: node_type.into(),
            properties: HashMap::new(),
            children: Vec::new(),
        }
    }

    pub fn with_property(mut self, key: impl Into<String>, value: Variant) -> Self {
        self.properties.insert(key.into(), value);
        self
    }

    pub fn with_child(mut self, child: ImportedSceneNode) -> Self {
        self.children.push(child);
        self
    }

    /// Total number of nodes in this subtree (including self).
    pub fn node_count(&self) -> usize {
        1 + self.children.iter().map(|c| c.node_count()).sum::<usize>()
    }
}

/// Result of a scene format import.
#[derive(Debug, Clone)]
pub struct ImportedScene {
    /// Root node of the imported scene hierarchy.
    pub root: ImportedSceneNode,
    /// Animations extracted during import (name → data placeholder).
    pub animations: Vec<ImportedAnimation>,
    /// Meshes/materials discovered during import.
    pub mesh_count: usize,
    /// Any warnings generated during import.
    pub warnings: Vec<String>,
}

/// An animation extracted from an imported scene file.
#[derive(Debug, Clone)]
pub struct ImportedAnimation {
    pub name: String,
    pub duration: f64,
    pub loop_mode: AnimationLoopMode,
}

/// Loop mode for imported animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnimationLoopMode {
    #[default]
    None,
    Linear,
    PingPong,
}

/// Trait for custom scene format importers.
///
/// Mirrors Godot's `EditorSceneFormatImporter` — plugins implement this
/// to add support for importing additional 3D scene formats (glTF, FBX,
/// Collada, OBJ, etc.) into the editor.
pub trait EditorSceneFormatImporter: std::fmt::Debug + Send + Sync {
    /// File extensions this importer handles (without leading dot, lowercase).
    fn get_extensions(&self) -> &[&str];

    /// Returns default import options for this format.
    fn get_default_options(&self) -> SceneImportOptions {
        SceneImportOptions::new()
    }

    /// Import a scene file with the given options.
    fn import_scene(
        &self,
        path: &Path,
        options: &SceneImportOptions,
    ) -> EngineResult<ImportedScene>;

    /// Human-readable name.
    fn importer_name(&self) -> &str;
}

/// Trait for post-import processing of scenes.
///
/// Mirrors Godot's `EditorScenePostImport`. Runs after the scene format
/// importer finishes, allowing modifications to the imported scene tree
/// (e.g. renaming nodes, adding metadata, stripping unused animations).
pub trait EditorScenePostImport: std::fmt::Debug + Send + Sync {
    /// Post-process the imported scene. Return the (possibly modified) scene.
    fn post_import(&self, scene: ImportedScene) -> EngineResult<ImportedScene>;

    /// Human-readable name of this post-import step.
    fn name(&self) -> &str;
}

/// Registry for scene format importers and post-import hooks.
#[derive(Debug, Default)]
pub struct SceneFormatImporterRegistry {
    importers: Vec<Box<dyn EditorSceneFormatImporter>>,
    post_importers: Vec<Box<dyn EditorScenePostImport>>,
}

impl SceneFormatImporterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a scene format importer.
    pub fn register_importer(&mut self, importer: Box<dyn EditorSceneFormatImporter>) {
        tracing::debug!("Registered scene format importer: {}", importer.importer_name());
        self.importers.push(importer);
    }

    /// Register a post-import hook.
    pub fn register_post_import(&mut self, post: Box<dyn EditorScenePostImport>) {
        tracing::debug!("Registered scene post-import: {}", post.name());
        self.post_importers.push(post);
    }

    /// Unregister a scene format importer by name.
    pub fn unregister_importer(&mut self, name: &str) -> bool {
        let before = self.importers.len();
        self.importers.retain(|i| i.importer_name() != name);
        self.importers.len() < before
    }

    /// Unregister a post-import hook by name.
    pub fn unregister_post_import(&mut self, name: &str) -> bool {
        let before = self.post_importers.len();
        self.post_importers.retain(|p| p.name() != name);
        self.post_importers.len() < before
    }

    /// Returns all file extensions supported by registered importers.
    pub fn supported_extensions(&self) -> Vec<&str> {
        let mut exts: Vec<&str> = self
            .importers
            .iter()
            .flat_map(|i| i.get_extensions().iter().copied())
            .collect();
        exts.sort();
        exts.dedup();
        exts
    }

    /// Find an importer that can handle the given file extension.
    pub fn find_importer(&self, extension: &str) -> Option<&dyn EditorSceneFormatImporter> {
        let ext_lower = extension.to_ascii_lowercase();
        self.importers
            .iter()
            .find(|i| i.get_extensions().iter().any(|e| *e == ext_lower))
            .map(|i| i.as_ref())
    }

    /// Import a scene file: find the right importer, run it, then apply
    /// all registered post-import hooks in order.
    pub fn import_scene(
        &self,
        path: &Path,
        options: Option<&SceneImportOptions>,
    ) -> EngineResult<ImportedScene> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let importer = self.find_importer(ext).ok_or_else(|| {
            EngineError::NotFound(format!(
                "no scene format importer for extension '{ext}'"
            ))
        })?;

        let opts = match options {
            Some(o) => o.clone(),
            None => importer.get_default_options(),
        };

        let mut scene = importer.import_scene(path, &opts)?;

        // Run post-import hooks.
        for post in &self.post_importers {
            scene = post.post_import(scene)?;
        }

        Ok(scene)
    }

    /// Number of registered importers.
    pub fn importer_count(&self) -> usize {
        self.importers.len()
    }

    /// Number of registered post-import hooks.
    pub fn post_import_count(&self) -> usize {
        self.post_importers.len()
    }
}

/// Built-in glTF scene importer.
#[derive(Debug)]
pub struct GltfSceneImporter;

impl EditorSceneFormatImporter for GltfSceneImporter {
    fn get_extensions(&self) -> &[&str] {
        &["gltf", "glb"]
    }

    fn get_default_options(&self) -> SceneImportOptions {
        let mut opts = SceneImportOptions::new();
        opts.set("animation/import", Variant::Bool(true));
        opts.set("meshes/ensure_tangents", Variant::Bool(true));
        opts.set("nodes/apply_root_scale", Variant::Bool(true));
        opts
    }

    fn import_scene(
        &self,
        path: &Path,
        options: &SceneImportOptions,
    ) -> EngineResult<ImportedScene> {
        if !path.exists() {
            return Err(EngineError::NotFound(format!(
                "file not found: {}",
                path.display()
            )));
        }

        let content = std::fs::read(path).map_err(EngineError::Io)?;

        // Determine root name from filename.
        let root_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Scene")
            .to_string();

        let mut root = ImportedSceneNode::new(&root_name, "Node3D");
        root.properties
            .insert("transform/origin".into(), Variant::String("(0, 0, 0)".into()));

        // Stub: create a mesh child to represent imported geometry.
        let mesh_node = ImportedSceneNode::new(
            format!("{root_name}_Mesh"),
            "MeshInstance3D",
        );
        root.children.push(mesh_node);

        // Stub: extract animation if enabled.
        let import_anims = options
            .get("animation/import")
            .and_then(|v| match v {
                Variant::Bool(b) => Some(*b),
                _ => None,
            })
            .unwrap_or(true);

        let animations = if import_anims {
            // Stub: detect animations from file content length as a placeholder.
            if content.len() > 100 {
                vec![ImportedAnimation {
                    name: "default".into(),
                    duration: 1.0,
                    loop_mode: AnimationLoopMode::None,
                }]
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        Ok(ImportedScene {
            root,
            animations,
            mesh_count: 1,
            warnings: vec![],
        })
    }

    fn importer_name(&self) -> &str {
        "GltfSceneImporter"
    }
}

/// Built-in OBJ (Wavefront) scene importer.
#[derive(Debug)]
pub struct ObjSceneImporter;

impl EditorSceneFormatImporter for ObjSceneImporter {
    fn get_extensions(&self) -> &[&str] {
        &["obj"]
    }

    fn import_scene(
        &self,
        path: &Path,
        _options: &SceneImportOptions,
    ) -> EngineResult<ImportedScene> {
        if !path.exists() {
            return Err(EngineError::NotFound(format!(
                "file not found: {}",
                path.display()
            )));
        }

        let root_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("ObjScene")
            .to_string();

        let root = ImportedSceneNode::new(&root_name, "Node3D")
            .with_child(ImportedSceneNode::new(
                format!("{root_name}_Mesh"),
                "MeshInstance3D",
            ));

        Ok(ImportedScene {
            root,
            animations: vec![],
            mesh_count: 1,
            warnings: vec![],
        })
    }

    fn importer_name(&self) -> &str {
        "ObjSceneImporter"
    }
}

/// Built-in FBX scene importer stub.
#[derive(Debug)]
pub struct FbxSceneImporter;

impl EditorSceneFormatImporter for FbxSceneImporter {
    fn get_extensions(&self) -> &[&str] {
        &["fbx"]
    }

    fn get_default_options(&self) -> SceneImportOptions {
        let mut opts = SceneImportOptions::new();
        opts.set("animation/import", Variant::Bool(true));
        opts.set("meshes/generate_lods", Variant::Bool(false));
        opts
    }

    fn import_scene(
        &self,
        path: &Path,
        options: &SceneImportOptions,
    ) -> EngineResult<ImportedScene> {
        if !path.exists() {
            return Err(EngineError::NotFound(format!(
                "file not found: {}",
                path.display()
            )));
        }

        let root_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("FbxScene")
            .to_string();

        let mut root = ImportedSceneNode::new(&root_name, "Node3D");
        root.children.push(ImportedSceneNode::new(
            format!("{root_name}_Mesh"),
            "MeshInstance3D",
        ));

        // Skeleton stub for FBX files (common for character models).
        root.children.push(ImportedSceneNode::new(
            format!("{root_name}_Skeleton"),
            "Skeleton3D",
        ));

        let import_anims = options
            .get("animation/import")
            .and_then(|v| match v {
                Variant::Bool(b) => Some(*b),
                _ => None,
            })
            .unwrap_or(true);

        let animations = if import_anims {
            vec![ImportedAnimation {
                name: "idle".into(),
                duration: 2.0,
                loop_mode: AnimationLoopMode::Linear,
            }]
        } else {
            vec![]
        };

        Ok(ImportedScene {
            root,
            animations,
            mesh_count: 1,
            warnings: vec![],
        })
    }

    fn importer_name(&self) -> &str {
        "FbxSceneImporter"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn make_pipeline() -> ImportPipeline {
        let mut pipeline = ImportPipeline::new();
        pipeline.register(Box::new(TresImporter));
        pipeline.register(Box::new(TscnImporter));
        pipeline
    }

    #[test]
    fn tres_importer_can_import() {
        let imp = TresImporter;
        assert!(imp.can_import("tres"));
        assert!(imp.can_import("TRES"));
        assert!(!imp.can_import("tscn"));
        assert!(!imp.can_import("png"));
    }

    #[test]
    fn tscn_importer_can_import() {
        let imp = TscnImporter;
        assert!(imp.can_import("tscn"));
        assert!(imp.can_import("TSCN"));
        assert!(!imp.can_import("tres"));
    }

    #[test]
    fn import_tres_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.tres");
        std::fs::write(&path, "[gd_resource]\n").unwrap();

        let imp = TresImporter;
        let result = imp.import(&path).unwrap();
        assert_eq!(result.resource_type, "Resource");
        assert_eq!(result.source_path, path);
    }

    #[test]
    fn import_tscn_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("main.tscn");
        std::fs::write(&path, "[gd_scene]\n").unwrap();

        let imp = TscnImporter;
        let result = imp.import(&path).unwrap();
        assert_eq!(result.resource_type, "PackedScene");
    }

    #[test]
    fn import_nonexistent_file_fails() {
        let imp = TresImporter;
        let result = imp.import(Path::new("/nonexistent/file.tres"));
        assert!(result.is_err());
    }

    #[test]
    fn pipeline_register_and_count() {
        let pipeline = make_pipeline();
        assert_eq!(pipeline.importer_count(), 2);
    }

    #[test]
    fn pipeline_scan_directory() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.tscn"), "scene").unwrap();
        std::fs::write(dir.path().join("b.tres"), "resource").unwrap();
        std::fs::write(dir.path().join("c.png"), "image").unwrap();

        let pipeline = make_pipeline();
        let found = pipeline.scan_directory(dir.path()).unwrap();
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn pipeline_import_file_tracks_state() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("level.tscn");
        std::fs::write(&path, "[gd_scene]\n").unwrap();

        let mut pipeline = make_pipeline();
        let result = pipeline.import_file(&path).unwrap();
        assert!(result.is_some());
        assert!(matches!(
            pipeline.get_state(&path),
            Some(ImportState::Imported { .. })
        ));
    }

    #[test]
    fn pipeline_skips_unchanged_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("level.tscn");
        std::fs::write(&path, "[gd_scene]\n").unwrap();

        let mut pipeline = make_pipeline();
        let first = pipeline.import_file(&path).unwrap();
        assert!(first.is_some());

        // Second import with same content should be skipped.
        let second = pipeline.import_file(&path).unwrap();
        assert!(second.is_none());
    }

    #[test]
    fn pipeline_reimports_changed_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("level.tscn");
        std::fs::write(&path, "[gd_scene]\n").unwrap();

        let mut pipeline = make_pipeline();
        pipeline.import_file(&path).unwrap();

        // Modify the file.
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        f.write_all(b"[gd_scene format=3]\n").unwrap();

        let result = pipeline.import_file(&path).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn pipeline_mark_pending() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("level.tscn");
        std::fs::write(&path, "[gd_scene]\n").unwrap();

        let mut pipeline = make_pipeline();
        pipeline.import_file(&path).unwrap();
        pipeline.mark_pending(&path);
        assert_eq!(pipeline.get_state(&path), Some(&ImportState::Pending));
    }

    #[test]
    fn pipeline_no_importer_for_extension() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("image.png");
        std::fs::write(&path, "PNG data").unwrap();

        let mut pipeline = make_pipeline();
        let result = pipeline.import_file(&path);
        assert!(result.is_err());
    }

    // ---------------------------------------------------------------
    // EditorSceneFormatImporter tests
    // ---------------------------------------------------------------

    fn make_scene_registry() -> SceneFormatImporterRegistry {
        let mut reg = SceneFormatImporterRegistry::new();
        reg.register_importer(Box::new(GltfSceneImporter));
        reg.register_importer(Box::new(ObjSceneImporter));
        reg.register_importer(Box::new(FbxSceneImporter));
        reg
    }

    #[test]
    fn scene_registry_register_and_count() {
        let reg = make_scene_registry();
        assert_eq!(reg.importer_count(), 3);
        assert_eq!(reg.post_import_count(), 0);
    }

    #[test]
    fn scene_registry_supported_extensions() {
        let reg = make_scene_registry();
        let exts = reg.supported_extensions();
        assert!(exts.contains(&"gltf"));
        assert!(exts.contains(&"glb"));
        assert!(exts.contains(&"obj"));
        assert!(exts.contains(&"fbx"));
    }

    #[test]
    fn scene_registry_find_importer() {
        let reg = make_scene_registry();
        assert!(reg.find_importer("gltf").is_some());
        assert!(reg.find_importer("glb").is_some());
        assert!(reg.find_importer("obj").is_some());
        assert!(reg.find_importer("fbx").is_some());
        assert!(reg.find_importer("png").is_none());
    }

    #[test]
    fn scene_registry_find_importer_case_insensitive() {
        let reg = make_scene_registry();
        // Extensions are stored lowercase; search normalizes.
        assert!(reg.find_importer("GLTF").is_some());
        assert!(reg.find_importer("OBJ").is_some());
    }

    #[test]
    fn gltf_import_scene() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("model.gltf");
        // Write enough content to trigger animation detection (>100 bytes).
        std::fs::write(&path, "x".repeat(200)).unwrap();

        let reg = make_scene_registry();
        let scene = reg.import_scene(&path, None).unwrap();
        assert_eq!(scene.root.name, "model");
        assert_eq!(scene.root.node_type, "Node3D");
        assert_eq!(scene.mesh_count, 1);
        assert_eq!(scene.root.children.len(), 1);
        assert_eq!(scene.root.children[0].node_type, "MeshInstance3D");
        assert_eq!(scene.animations.len(), 1);
        assert_eq!(scene.animations[0].name, "default");
    }

    #[test]
    fn gltf_import_no_animations() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("static.glb");
        std::fs::write(&path, "x".repeat(200)).unwrap();

        let mut opts = SceneImportOptions::new();
        opts.set("animation/import", Variant::Bool(false));

        let reg = make_scene_registry();
        let scene = reg.import_scene(&path, Some(&opts)).unwrap();
        assert!(scene.animations.is_empty());
    }

    #[test]
    fn obj_import_scene() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cube.obj");
        std::fs::write(&path, "v 0 0 0\nv 1 0 0\n").unwrap();

        let reg = make_scene_registry();
        let scene = reg.import_scene(&path, None).unwrap();
        assert_eq!(scene.root.name, "cube");
        assert!(scene.animations.is_empty());
        assert_eq!(scene.root.node_count(), 2); // root + mesh child
    }

    #[test]
    fn fbx_import_scene_with_skeleton() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("character.fbx");
        std::fs::write(&path, "FBX binary data stub").unwrap();

        let reg = make_scene_registry();
        let scene = reg.import_scene(&path, None).unwrap();
        assert_eq!(scene.root.name, "character");
        assert_eq!(scene.root.children.len(), 2);
        // Should have MeshInstance3D and Skeleton3D children.
        let types: Vec<&str> = scene.root.children.iter().map(|c| c.node_type.as_str()).collect();
        assert!(types.contains(&"MeshInstance3D"));
        assert!(types.contains(&"Skeleton3D"));
        assert_eq!(scene.animations.len(), 1);
        assert_eq!(scene.animations[0].loop_mode, AnimationLoopMode::Linear);
    }

    #[test]
    fn scene_registry_no_importer_for_extension() {
        let reg = make_scene_registry();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("model.abc");
        std::fs::write(&path, "data").unwrap();
        let result = reg.import_scene(&path, None);
        assert!(result.is_err());
    }

    #[test]
    fn scene_import_nonexistent_file() {
        let reg = make_scene_registry();
        let result = reg.import_scene(Path::new("/nonexistent/model.gltf"), None);
        assert!(result.is_err());
    }

    #[test]
    fn scene_import_options_get_set() {
        let mut opts = SceneImportOptions::new();
        assert!(opts.get("foo").is_none());
        opts.set("foo", Variant::Int(42));
        assert_eq!(opts.get("foo"), Some(&Variant::Int(42)));
    }

    #[test]
    fn gltf_default_options() {
        let imp = GltfSceneImporter;
        let opts = imp.get_default_options();
        assert_eq!(opts.get("animation/import"), Some(&Variant::Bool(true)));
        assert_eq!(opts.get("meshes/ensure_tangents"), Some(&Variant::Bool(true)));
    }

    #[test]
    fn imported_scene_node_builder() {
        let node = ImportedSceneNode::new("Root", "Node3D")
            .with_property("visible", Variant::Bool(true))
            .with_child(ImportedSceneNode::new("Child", "MeshInstance3D"));
        assert_eq!(node.name, "Root");
        assert_eq!(node.children.len(), 1);
        assert_eq!(node.properties.len(), 1);
        assert_eq!(node.node_count(), 2);
    }

    #[test]
    fn imported_scene_node_deep_count() {
        let node = ImportedSceneNode::new("A", "Node3D")
            .with_child(
                ImportedSceneNode::new("B", "Node3D")
                    .with_child(ImportedSceneNode::new("C", "Node3D")),
            )
            .with_child(ImportedSceneNode::new("D", "Node3D"));
        assert_eq!(node.node_count(), 4);
    }

    #[test]
    fn scene_registry_unregister_importer() {
        let mut reg = make_scene_registry();
        assert_eq!(reg.importer_count(), 3);
        assert!(reg.unregister_importer("ObjSceneImporter"));
        assert_eq!(reg.importer_count(), 2);
        assert!(reg.find_importer("obj").is_none());
        // Unregistering again returns false.
        assert!(!reg.unregister_importer("ObjSceneImporter"));
    }

    #[derive(Debug)]
    struct TestPostImport;

    impl EditorScenePostImport for TestPostImport {
        fn post_import(&self, mut scene: ImportedScene) -> EngineResult<ImportedScene> {
            scene.warnings.push("post-import ran".into());
            scene.root.properties.insert("_imported".into(), Variant::Bool(true));
            Ok(scene)
        }

        fn name(&self) -> &str {
            "TestPostImport"
        }
    }

    #[test]
    fn scene_post_import_hook_runs() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("model.gltf");
        std::fs::write(&path, "gltf data placeholder content bytes").unwrap();

        let mut reg = SceneFormatImporterRegistry::new();
        reg.register_importer(Box::new(GltfSceneImporter));
        reg.register_post_import(Box::new(TestPostImport));
        assert_eq!(reg.post_import_count(), 1);

        let scene = reg.import_scene(&path, None).unwrap();
        assert_eq!(scene.warnings, vec!["post-import ran"]);
        assert_eq!(
            scene.root.properties.get("_imported"),
            Some(&Variant::Bool(true))
        );
    }

    #[test]
    fn scene_post_import_unregister() {
        let mut reg = SceneFormatImporterRegistry::new();
        reg.register_post_import(Box::new(TestPostImport));
        assert_eq!(reg.post_import_count(), 1);
        assert!(reg.unregister_post_import("TestPostImport"));
        assert_eq!(reg.post_import_count(), 0);
    }

    #[test]
    fn animation_loop_mode_default() {
        assert_eq!(AnimationLoopMode::default(), AnimationLoopMode::None);
    }
}
