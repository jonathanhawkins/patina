//! Resource import pipeline.
//!
//! Provides the [`ResourceImporter`] trait for pluggable import backends,
//! an [`ImportPipeline`] that orchestrates scanning and importing, and
//! built-in importers for `.tres` and `.tscn` files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gdcore::error::{EngineError, EngineResult};

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

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

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
}
