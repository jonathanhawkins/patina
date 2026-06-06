//! FileSystem dock **import pipeline hooks** (pat-3b8zp).
//!
//! Importable assets (textures, audio, …) are tracked with the source version
//! they were last imported from and a set of per-asset import settings. When a
//! source file changes — or its import settings are edited — the asset is marked
//! **stale** so the dock can show that it needs re-importing; processing the
//! stale set re-imports each asset and clears it back to **imported**. This is
//! the headless model the dock drives: edit → stale → re-import → imported.

use std::collections::{BTreeMap, HashMap};

/// The import state of an asset, as reflected in the dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportStatus {
    /// Registered but never imported.
    NotImported,
    /// Imported and up to date with its source and settings.
    Imported,
    /// Source or settings changed since the last import; needs re-importing.
    Stale,
    /// The last import attempt failed.
    Failed,
}

/// One tracked importable asset.
#[derive(Debug, Clone)]
pub struct ImportedAsset {
    /// Source file path.
    pub path: String,
    /// Current source version (bumped when the file changes).
    pub source_version: u64,
    /// The source version the current import reflects, if imported.
    pub imported_version: Option<u64>,
    /// Import status as shown in the dock.
    pub status: ImportStatus,
    /// Editable per-asset import settings.
    pub settings: BTreeMap<String, String>,
}

impl ImportedAsset {
    /// Whether this asset needs (re-)importing.
    pub fn needs_import(&self) -> bool {
        matches!(self.status, ImportStatus::NotImported | ImportStatus::Stale)
    }
}

/// Tracks importable assets and drives re-import on change.
#[derive(Debug, Clone, Default)]
pub struct ImportPipeline {
    assets: HashMap<String, ImportedAsset>,
    reimports: u64,
}

impl ImportPipeline {
    /// Creates an empty pipeline.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an importable asset at `path` with source `version`. A freshly
    /// registered asset is `NotImported` (needs an initial import).
    pub fn register(&mut self, path: impl Into<String>, version: u64) -> &ImportedAsset {
        let path = path.into();
        let asset = ImportedAsset {
            path: path.clone(),
            source_version: version,
            imported_version: None,
            status: ImportStatus::NotImported,
            settings: BTreeMap::new(),
        };
        self.assets.entry(path.clone()).or_insert(asset);
        &self.assets[&path]
    }

    /// The tracked asset at `path`, if registered.
    pub fn asset(&self, path: &str) -> Option<&ImportedAsset> {
        self.assets.get(path)
    }

    /// The import status of `path`, if registered.
    pub fn status(&self, path: &str) -> Option<ImportStatus> {
        self.assets.get(path).map(|a| a.status)
    }

    /// Whether `path` needs (re-)importing.
    pub fn is_stale(&self, path: &str) -> bool {
        self.assets.get(path).is_some_and(|a| a.needs_import())
    }

    /// The number of re-imports performed over the pipeline's lifetime.
    pub fn reimport_count(&self) -> u64 {
        self.reimports
    }

    /// Reads a per-asset import setting.
    pub fn setting(&self, path: &str, key: &str) -> Option<&str> {
        self.assets
            .get(path)?
            .settings
            .get(key)
            .map(String::as_str)
    }

    /// Edits a per-asset import setting, marking the asset stale (a settings
    /// change requires re-importing). Returns whether the asset exists.
    pub fn set_setting(
        &mut self,
        path: &str,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> bool {
        match self.assets.get_mut(path) {
            Some(asset) => {
                asset.settings.insert(key.into(), value.into());
                if asset.status == ImportStatus::Imported {
                    asset.status = ImportStatus::Stale;
                }
                true
            }
            None => false,
        }
    }

    /// Notifies the pipeline that `path`'s source changed to `new_version`. If
    /// the new version differs from what's imported, the asset is marked stale.
    /// Returns whether the asset became stale.
    pub fn source_changed(&mut self, path: &str, new_version: u64) -> bool {
        match self.assets.get_mut(path) {
            Some(asset) => {
                asset.source_version = new_version;
                if asset.imported_version != Some(new_version) {
                    if asset.status == ImportStatus::Imported {
                        asset.status = ImportStatus::Stale;
                    }
                    return asset.needs_import();
                }
                false
            }
            None => false,
        }
    }

    /// Re-imports a single asset: marks it imported at the current source
    /// version. Returns whether the asset exists.
    pub fn reimport(&mut self, path: &str) -> bool {
        match self.assets.get_mut(path) {
            Some(asset) => {
                asset.imported_version = Some(asset.source_version);
                asset.status = ImportStatus::Imported;
                self.reimports += 1;
                true
            }
            None => false,
        }
    }

    /// Re-imports every asset that needs it (the dock's "process stale imports"
    /// pass). Returns how many were re-imported.
    pub fn process_stale(&mut self) -> usize {
        let stale: Vec<String> = self
            .assets
            .values()
            .filter(|a| a.needs_import())
            .map(|a| a.path.clone())
            .collect();
        for path in &stale {
            self.reimport(path);
        }
        stale.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-3b8zp): changing an imported asset triggers a re-import
    /// and the dock reflects import status; import settings are editable per
    /// asset.
    #[test]
    fn fs_dock_import_hooks_reimport_on_change() {
        let mut pipe = ImportPipeline::new();

        // Register an importable texture — initially not imported.
        pipe.register("art/hero.png", 1);
        assert_eq!(pipe.status("art/hero.png"), Some(ImportStatus::NotImported));
        assert!(pipe.is_stale("art/hero.png"));

        // Processing stale imports imports it; the dock now shows Imported.
        assert_eq!(pipe.process_stale(), 1);
        assert_eq!(pipe.status("art/hero.png"), Some(ImportStatus::Imported));
        assert_eq!(pipe.asset("art/hero.png").unwrap().imported_version, Some(1));
        assert_eq!(pipe.reimport_count(), 1);
        assert!(!pipe.is_stale("art/hero.png"));

        // Import settings are editable per asset, and editing them marks the
        // asset stale (re-import required).
        assert!(pipe.set_setting("art/hero.png", "filter", "nearest"));
        assert_eq!(pipe.setting("art/hero.png", "filter"), Some("nearest"));
        assert_eq!(pipe.status("art/hero.png"), Some(ImportStatus::Stale));

        // Re-processing clears it back to Imported.
        assert_eq!(pipe.process_stale(), 1);
        assert_eq!(pipe.status("art/hero.png"), Some(ImportStatus::Imported));
        assert_eq!(pipe.reimport_count(), 2);

        // Changing the source file triggers a re-import: status goes Stale.
        assert!(pipe.source_changed("art/hero.png", 2));
        assert_eq!(pipe.status("art/hero.png"), Some(ImportStatus::Stale));
        assert!(pipe.is_stale("art/hero.png"));

        // After processing, it's imported at the new version.
        assert_eq!(pipe.process_stale(), 1);
        assert_eq!(pipe.status("art/hero.png"), Some(ImportStatus::Imported));
        assert_eq!(pipe.asset("art/hero.png").unwrap().imported_version, Some(2));
        assert_eq!(pipe.reimport_count(), 3);

        // A no-op source change (same version already imported) is not stale.
        assert!(!pipe.source_changed("art/hero.png", 2));
        assert!(!pipe.is_stale("art/hero.png"));

        // Unregistered assets report nothing.
        assert_eq!(pipe.status("art/missing.png"), None);
        assert!(!pipe.set_setting("art/missing.png", "k", "v"));
        assert!(!pipe.source_changed("art/missing.png", 9));
    }

    /// `process_stale` re-imports every pending asset and reports the count.
    #[test]
    fn process_stale_imports_all_pending() {
        let mut pipe = ImportPipeline::new();
        pipe.register("a.png", 1);
        pipe.register("b.wav", 1);
        pipe.register("c.png", 1);
        // All three are pending initial import.
        assert_eq!(pipe.process_stale(), 3);
        assert_eq!(pipe.reimport_count(), 3);
        // Nothing stale now → a second pass is a no-op.
        assert_eq!(pipe.process_stale(), 0);

        // Touch two of them; only those re-import.
        pipe.source_changed("a.png", 2);
        pipe.set_setting("c.png", "mono", "true");
        assert_eq!(pipe.process_stale(), 2);
        assert_eq!(pipe.reimport_count(), 5);
    }
}
