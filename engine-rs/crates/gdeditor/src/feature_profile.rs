//! Godot's `EditorFeatureProfile` — which editor features, classes, class
//! editors and properties are hidden (pat-s3rrv.225).
//!
//! Before this the menu entry existed and did nothing: `editor_menu.rs` mapped
//! `MenuAction::EditorFeatureProfile` to the bare string
//! `OpenDialog("Manage Feature Profiles")` and nothing consumed it.
//!
//! Ground truth: `upstream/godot/editor/settings/editor_feature_profile.{h,cpp}`
//! and `doc/classes/EditorFeatureProfile.xml`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Godot's `EditorFeatureProfile.Feature`.
///
/// ⚠️ **Eleven features, not the nine the bead lists.** `SIGNALS_DOCK` and
/// `GROUPS_DOCK` exist in 4.6.1 and were omitted from the bead's enumeration;
/// `NODE_DOCK` is deprecated upstream but still occupies its ordinal, so
/// dropping it would renumber everything after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EditorFeature {
    /// `FEATURE_3D`.
    ThreeD,
    /// `FEATURE_SCRIPT`.
    Script,
    /// `FEATURE_ASSET_LIB`.
    AssetLib,
    /// `FEATURE_SCENE_TREE`.
    SceneTree,
    /// `FEATURE_NODE_DOCK` — deprecated upstream, ordinal retained.
    NodeDock,
    /// `FEATURE_FILESYSTEM_DOCK`.
    FilesystemDock,
    /// `FEATURE_IMPORT_DOCK`.
    ImportDock,
    /// `FEATURE_HISTORY_DOCK`.
    HistoryDock,
    /// `FEATURE_GAME`.
    Game,
    /// `FEATURE_SIGNALS_DOCK`.
    SignalsDock,
    /// `FEATURE_GROUPS_DOCK`.
    GroupsDock,
}

impl EditorFeature {
    /// Every feature, in Godot's ordinal order.
    pub const ALL: [EditorFeature; 11] = [
        EditorFeature::ThreeD,
        EditorFeature::Script,
        EditorFeature::AssetLib,
        EditorFeature::SceneTree,
        EditorFeature::NodeDock,
        EditorFeature::FilesystemDock,
        EditorFeature::ImportDock,
        EditorFeature::HistoryDock,
        EditorFeature::Game,
        EditorFeature::SignalsDock,
        EditorFeature::GroupsDock,
    ];

    /// The integer a script passes to `set_disabled_feature`.
    pub fn as_godot(self) -> i64 {
        Self::ALL.iter().position(|f| *f == self).unwrap_or(0) as i64
    }

    /// Maps Godot's integer back, rejecting anything out of range.
    pub fn from_godot(value: i64) -> Option<Self> {
        usize::try_from(value)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
    }

    /// `EditorFeatureProfile.get_feature_name()`.
    ///
    /// Strings transcribed from `editor_feature_profile.cpp:45-58`; they are
    /// user-facing labels, so they are not derivable from the enum name
    /// ("3D Editor", not "3D"; "Game View", not "Game").
    pub fn name(self) -> &'static str {
        match self {
            EditorFeature::ThreeD => "3D Editor",
            EditorFeature::Script => "Script Editor",
            EditorFeature::AssetLib => "Asset Library",
            EditorFeature::SceneTree => "Scene Tree Editing",
            EditorFeature::NodeDock => "Node Dock (deprecated)",
            EditorFeature::FilesystemDock => "FileSystem Dock",
            EditorFeature::ImportDock => "Import Dock",
            EditorFeature::HistoryDock => "History Dock",
            EditorFeature::Game => "Game View",
            EditorFeature::SignalsDock => "Signals Dock",
            EditorFeature::GroupsDock => "Groups Dock",
        }
    }
}

/// A saved editor feature profile.
///
/// The on-disk shape is Godot's: a `.profile` JSON object with `type`,
/// `disabled_classes`, `disabled_editors`, `disabled_features` and
/// `disabled_properties` (`save_to_file`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorFeatureProfile {
    /// Always `"feature_profile"`; Godot writes and checks it.
    #[serde(default = "profile_type_tag")]
    pub r#type: String,
    /// Classes hidden from Create Node and the docks.
    #[serde(default)]
    pub disabled_classes: BTreeSet<String>,
    /// Classes whose custom editor is hidden while the class itself stays.
    #[serde(default)]
    pub disabled_editors: BTreeSet<String>,
    /// `"Class:property"` entries hidden in the inspector.
    #[serde(default)]
    pub disabled_properties: BTreeSet<String>,
    /// Feature *names*, matching Godot's on-disk form.
    #[serde(default)]
    pub disabled_features: BTreeSet<String>,
}

fn profile_type_tag() -> String {
    "feature_profile".to_string()
}

impl EditorFeatureProfile {
    /// An empty profile — nothing disabled, which is the editor's default.
    pub fn new() -> Self {
        Self {
            r#type: profile_type_tag(),
            ..Default::default()
        }
    }

    /// `set_disabled_class(class, disabled)`.
    pub fn set_disabled_class(&mut self, class_name: &str, disabled: bool) {
        toggle(&mut self.disabled_classes, class_name, disabled);
    }

    /// `is_disabled_class(class)`.
    pub fn is_disabled_class(&self, class_name: &str) -> bool {
        self.disabled_classes.contains(class_name)
    }

    /// `set_disabled_class_editor(class, disabled)`.
    pub fn set_disabled_class_editor(&mut self, class_name: &str, disabled: bool) {
        toggle(&mut self.disabled_editors, class_name, disabled);
    }

    /// `is_disabled_class_editor(class)`.
    pub fn is_disabled_class_editor(&self, class_name: &str) -> bool {
        self.disabled_editors.contains(class_name)
    }

    /// `set_disabled_property("Class:property", disabled)`.
    pub fn set_disabled_property(&mut self, path: &str, disabled: bool) {
        toggle(&mut self.disabled_properties, path, disabled);
    }

    /// `is_disabled_property("Class:property")`.
    pub fn is_disabled_property(&self, path: &str) -> bool {
        self.disabled_properties.contains(path)
    }

    /// `set_disabled_feature(feature, disabled)`.
    pub fn set_disabled_feature(&mut self, feature: EditorFeature, disabled: bool) {
        toggle(&mut self.disabled_features, feature.name(), disabled);
    }

    /// `is_disabled_feature(feature)`.
    pub fn is_disabled_feature(&self, feature: EditorFeature) -> bool {
        self.disabled_features.contains(feature.name())
    }

    /// Every feature still enabled — what the served editor should show.
    pub fn enabled_features(&self) -> Vec<EditorFeature> {
        EditorFeature::ALL
            .iter()
            .copied()
            .filter(|f| !self.is_disabled_feature(*f))
            .collect()
    }

    /// `save_to_file(path)`.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// `load_from_file(path)`.
    ///
    /// ⚠️ A file whose `type` is not `feature_profile` is refused. Godot checks
    /// the same tag; without it any JSON object would load as an empty profile
    /// and silently disable nothing, which looks identical to a working load.
    pub fn load_from_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let profile: Self = serde_json::from_str(&text)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if profile.r#type != profile_type_tag() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("not a feature profile: type = {:?}", profile.r#type),
            ));
        }
        Ok(profile)
    }
}

fn toggle(set: &mut BTreeSet<String>, key: &str, on: bool) {
    if on {
        set.insert(key.to_string());
    } else {
        set.remove(key);
    }
}

static ACTIVE: OnceLock<Mutex<Option<EditorFeatureProfile>>> = OnceLock::new();

fn slot() -> MutexGuard<'static, Option<EditorFeatureProfile>> {
    ACTIVE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Selects the profile the editor applies. `None` restores "nothing disabled".
pub fn set_active_profile(profile: Option<EditorFeatureProfile>) {
    *slot() = profile;
}

/// The profile currently applied, if any.
pub fn active_profile() -> Option<EditorFeatureProfile> {
    slot().clone()
}

/// Whether `class_name` is hidden by the active profile.
///
/// With no profile selected nothing is hidden, which is what keeps every
/// existing editor path behaving exactly as before.
pub fn is_class_disabled(class_name: &str) -> bool {
    active_profile().is_some_and(|p| p.is_disabled_class(class_name))
}

/// Whether `feature` is hidden by the active profile.
pub fn is_feature_disabled(feature: EditorFeature) -> bool {
    active_profile().is_some_and(|p| p.is_disabled_feature(feature))
}
