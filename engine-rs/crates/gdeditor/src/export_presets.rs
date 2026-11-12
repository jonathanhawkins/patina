//! **Export presets** (pat-2nng7).
//!
//! Manages the project's export presets: create / edit / delete a preset,
//! choose its target platform, set the resources and feature tags, and export
//! it to a target path — producing an artifact. The whole preset list persists
//! to `export_presets.cfg` (modeled here as a serialized round-trip).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A single export preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportPreset {
    /// Display name.
    pub name: String,
    /// Target platform (e.g. `"windows"`, `"linux"`, `"web"`).
    pub platform: String,
    /// Output path the export writes to.
    pub export_path: String,
    /// Feature tags enabled for this preset.
    pub features: Vec<String>,
    /// Resource paths included in the export.
    pub resources: Vec<String>,
}

impl ExportPreset {
    fn new(name: impl Into<String>, platform: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            platform: platform.into(),
            export_path: String::new(),
            features: Vec::new(),
            resources: Vec::new(),
        }
    }

    /// Renders the export artifact contents — a manifest of the preset.
    fn manifest(&self) -> String {
        let mut out = format!("platform={}\nname={}\n", self.platform, self.name);
        for f in &self.features {
            out.push_str(&format!("feature={}\n", f));
        }
        for r in &self.resources {
            out.push_str(&format!("resource={}\n", r));
        }
        out
    }
}

/// The project's export presets (the contents of `export_presets.cfg`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportPresets {
    presets: Vec<ExportPreset>,
}

impl ExportPresets {
    /// Creates an empty preset list.
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of presets.
    pub fn len(&self) -> usize {
        self.presets.len()
    }

    /// Whether there are no presets.
    pub fn is_empty(&self) -> bool {
        self.presets.is_empty()
    }

    /// A preset by index.
    pub fn get(&self, index: usize) -> Option<&ExportPreset> {
        self.presets.get(index)
    }

    /// A preset by name.
    pub fn by_name(&self, name: &str) -> Option<&ExportPreset> {
        self.presets.iter().find(|p| p.name == name)
    }

    /// Creates a new preset for `platform`, returning its index.
    pub fn create(&mut self, name: impl Into<String>, platform: impl Into<String>) -> usize {
        self.presets.push(ExportPreset::new(name, platform));
        self.presets.len() - 1
    }

    /// Removes the preset at `index`. Returns whether it existed.
    pub fn remove(&mut self, index: usize) -> bool {
        if index < self.presets.len() {
            self.presets.remove(index);
            true
        } else {
            false
        }
    }

    /// Renames the preset at `index`. Returns whether it exists.
    pub fn rename(&mut self, index: usize, name: impl Into<String>) -> bool {
        match self.presets.get_mut(index) {
            Some(p) => {
                p.name = name.into();
                true
            }
            None => false,
        }
    }

    /// Sets the preset's target platform.
    pub fn set_platform(&mut self, index: usize, platform: impl Into<String>) -> bool {
        match self.presets.get_mut(index) {
            Some(p) => {
                p.platform = platform.into();
                true
            }
            None => false,
        }
    }

    /// Sets the preset's output path.
    pub fn set_export_path(&mut self, index: usize, path: impl Into<String>) -> bool {
        match self.presets.get_mut(index) {
            Some(p) => {
                p.export_path = path.into();
                true
            }
            None => false,
        }
    }

    /// Adds a feature tag to the preset.
    pub fn add_feature(&mut self, index: usize, feature: impl Into<String>) -> bool {
        match self.presets.get_mut(index) {
            Some(p) => {
                p.features.push(feature.into());
                true
            }
            None => false,
        }
    }

    /// Adds an included resource to the preset.
    pub fn add_resource(&mut self, index: usize, resource: impl Into<String>) -> bool {
        match self.presets.get_mut(index) {
            Some(p) => {
                p.resources.push(resource.into());
                true
            }
            None => false,
        }
    }

    /// Exports the preset at `index` into `out` (a virtual filesystem),
    /// producing an artifact at the preset's `export_path`. Returns the artifact
    /// path, or `None` if the preset is missing or has no export path.
    pub fn export(&self, index: usize, out: &mut BTreeMap<String, String>) -> Option<String> {
        let preset = self.presets.get(index)?;
        if preset.export_path.is_empty() {
            return None;
        }
        out.insert(preset.export_path.clone(), preset.manifest());
        Some(preset.export_path.clone())
    }

    /// Serializes the presets to `export_presets.cfg` contents.
    pub fn to_cfg(&self) -> String {
        serde_json::to_string_pretty(self).expect("ExportPresets serializes")
    }

    /// Loads presets from `export_presets.cfg` contents.
    pub fn from_cfg(cfg: &str) -> serde_json::Result<Self> {
        serde_json::from_str(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-2nng7): creating a preset and exporting produces a target
    /// artifact and the preset persists in `export_presets.cfg`.
    #[test]
    fn systems_export_presets_create_and_export() {
        let mut presets = ExportPresets::new();

        // Create and configure a Windows preset.
        let win = presets.create("Windows Desktop", "windows");
        assert_eq!(presets.len(), 1);
        assert!(presets.set_export_path(win, "build/game.exe"));
        assert!(presets.add_feature(win, "64"));
        assert!(presets.add_resource(win, "res://main.tscn"));

        // Exporting produces a target artifact containing the preset's settings.
        let mut fs = BTreeMap::new();
        let artifact = presets.export(win, &mut fs).expect("exported");
        assert_eq!(artifact, "build/game.exe");
        let content = &fs["build/game.exe"];
        assert!(content.contains("platform=windows"));
        assert!(content.contains("feature=64"));
        assert!(content.contains("resource=res://main.tscn"));

        // The presets persist to and load from export_presets.cfg unchanged.
        let cfg = presets.to_cfg();
        let loaded = ExportPresets::from_cfg(&cfg).expect("loads");
        assert_eq!(loaded.len(), 1);
        let p = loaded.get(0).unwrap();
        assert_eq!(p.platform, "windows");
        assert_eq!(p.export_path, "build/game.exe");
        assert_eq!(loaded, presets);

        // Edit (rename, retarget platform) and delete operate on the list.
        let lin = presets.create("Linux", "linux");
        assert_eq!(presets.len(), 2);
        assert!(presets.rename(lin, "Linux/X11"));
        assert!(presets.set_platform(lin, "linuxbsd"));
        assert_eq!(presets.get(lin).unwrap().name, "Linux/X11");
        assert_eq!(presets.get(lin).unwrap().platform, "linuxbsd");
        assert!(presets.remove(lin));
        assert_eq!(presets.len(), 1);

        // A preset with no export path can't be exported.
        let web = presets.create("Web", "web");
        assert!(presets.export(web, &mut fs).is_none());
        // …and operations on a missing index return false.
        assert!(!presets.rename(99, "x"));
        assert!(!presets.remove(99));
    }
}
