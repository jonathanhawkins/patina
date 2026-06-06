//! Asset browser import settings panel for each resource type.
//!
//! Provides per-resource-type import configuration (texture compression,
//! audio format, scene import options, mesh LOD settings, etc.) with
//! a panel UI model for the asset browser sidebar.

use std::collections::HashMap;

/// Resource types that have configurable import settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportResourceType {
    Texture,
    Audio,
    Scene,
    Mesh,
    Font,
    Shader,
    Script,
}

impl ImportResourceType {
    pub fn all() -> &'static [ImportResourceType] {
        &[
            Self::Texture,
            Self::Audio,
            Self::Scene,
            Self::Mesh,
            Self::Font,
            Self::Shader,
            Self::Script,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Texture => "Texture",
            Self::Audio => "Audio",
            Self::Scene => "Scene",
            Self::Mesh => "Mesh",
            Self::Font => "Font",
            Self::Shader => "Shader",
            Self::Script => "Script",
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Self::Texture => "texture",
            Self::Audio => "audio",
            Self::Scene => "scene",
            Self::Mesh => "mesh",
            Self::Font => "font",
            Self::Shader => "shader",
            Self::Script => "script",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "texture" => Some(Self::Texture),
            "audio" => Some(Self::Audio),
            "scene" => Some(Self::Scene),
            "mesh" => Some(Self::Mesh),
            "font" => Some(Self::Font),
            "shader" => Some(Self::Shader),
            "script" => Some(Self::Script),
            _ => None,
        }
    }

    /// Infers the resource type from a file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "bmp" | "tga" | "webp" | "svg" | "exr" | "hdr" => {
                Some(Self::Texture)
            }
            "wav" | "ogg" | "mp3" | "flac" | "aiff" => Some(Self::Audio),
            "tscn" | "scn" | "glb" | "gltf" | "dae" | "fbx" | "obj" => Some(Self::Scene),
            "mesh" | "res" => Some(Self::Mesh),
            "ttf" | "otf" | "woff" | "woff2" | "fnt" => Some(Self::Font),
            "gdshader" | "shader" | "glsl" => Some(Self::Shader),
            "gd" | "gdscript" | "cs" => Some(Self::Script),
            _ => None,
        }
    }

    /// File extension patterns for this resource type.
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Texture => &[
                "png", "jpg", "jpeg", "bmp", "tga", "webp", "svg", "exr", "hdr",
            ],
            Self::Audio => &["wav", "ogg", "mp3", "flac"],
            Self::Scene => &["tscn", "scn", "glb", "gltf"],
            Self::Mesh => &["mesh", "obj"],
            Self::Font => &["ttf", "otf", "woff", "woff2"],
            Self::Shader => &["gdshader", "shader"],
            Self::Script => &["gd", "cs"],
        }
    }
}

/// Texture compression mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureCompression {
    Lossless,
    Lossy,
    VRAM,
    Uncompressed,
}

impl TextureCompression {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Lossless => "Lossless",
            Self::Lossy => "Lossy",
            Self::VRAM => "VRAM Compressed",
            Self::Uncompressed => "Uncompressed",
        }
    }

    pub fn all() -> &'static [TextureCompression] {
        &[Self::Lossless, Self::Lossy, Self::VRAM, Self::Uncompressed]
    }
}

/// Texture filter mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFilter {
    Nearest,
    Linear,
    NearestMipmap,
    LinearMipmap,
}

impl TextureFilter {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Nearest => "Nearest",
            Self::Linear => "Linear",
            Self::NearestMipmap => "Nearest (Mipmap)",
            Self::LinearMipmap => "Linear (Mipmap)",
        }
    }
}

/// Import settings for textures.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureImportSettings {
    pub compression: TextureCompression,
    pub lossy_quality: f32,
    pub filter: TextureFilter,
    pub mipmaps: bool,
    pub srgb: bool,
    pub max_size: u32,
    pub normal_map: bool,
}

impl Default for TextureImportSettings {
    fn default() -> Self {
        Self {
            compression: TextureCompression::VRAM,
            lossy_quality: 0.7,
            filter: TextureFilter::Linear,
            mipmaps: true,
            srgb: true,
            max_size: 0, // 0 = no limit
            normal_map: false,
        }
    }
}

/// Audio sample rate conversion mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormatMode {
    /// Keep original format.
    Original,
    /// Convert to Ogg Vorbis.
    OggVorbis,
    /// Convert to WAV (PCM).
    Wav,
}

impl AudioFormatMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Original => "Keep Original",
            Self::OggVorbis => "Ogg Vorbis",
            Self::Wav => "WAV (PCM)",
        }
    }
}

/// Import settings for audio.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioImportSettings {
    pub format: AudioFormatMode,
    pub quality: f32,
    pub loop_enabled: bool,
    pub loop_offset: f64,
    pub force_mono: bool,
    pub max_sample_rate: u32,
}

impl Default for AudioImportSettings {
    fn default() -> Self {
        Self {
            format: AudioFormatMode::Original,
            quality: 0.5,
            loop_enabled: false,
            loop_offset: 0.0,
            force_mono: false,
            max_sample_rate: 44100,
        }
    }
}

/// Import settings for scenes (tscn, glTF, etc.).
#[derive(Debug, Clone, PartialEq)]
pub struct SceneImportSettings {
    pub import_animations: bool,
    pub import_materials: bool,
    pub import_lights: bool,
    pub import_cameras: bool,
    pub mesh_compression: bool,
    pub generate_tangents: bool,
    pub scale_factor: f64,
    pub root_type: String,
}

impl Default for SceneImportSettings {
    fn default() -> Self {
        Self {
            import_animations: true,
            import_materials: true,
            import_lights: true,
            import_cameras: true,
            mesh_compression: true,
            generate_tangents: true,
            scale_factor: 1.0,
            root_type: "Node3D".into(),
        }
    }
}

/// Import settings for meshes.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshImportSettings {
    pub generate_lods: bool,
    pub lod_count: u8,
    pub lod_distance_ratio: f32,
    pub generate_collision: bool,
    pub optimize: bool,
    pub lightmap_uv2: bool,
}

impl Default for MeshImportSettings {
    fn default() -> Self {
        Self {
            generate_lods: true,
            lod_count: 3,
            lod_distance_ratio: 2.0,
            generate_collision: false,
            optimize: true,
            lightmap_uv2: false,
        }
    }
}

/// Import settings for fonts.
#[derive(Debug, Clone, PartialEq)]
pub struct FontImportSettings {
    pub default_size: u32,
    pub antialiased: bool,
    pub hinting: FontHinting,
    pub subpixel_positioning: bool,
    pub preload_sizes: Vec<u32>,
}

/// Font hinting mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontHinting {
    None,
    Light,
    Normal,
}

impl FontHinting {
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Light => "Light",
            Self::Normal => "Normal",
        }
    }
}

impl Default for FontImportSettings {
    fn default() -> Self {
        Self {
            default_size: 16,
            antialiased: true,
            hinting: FontHinting::Light,
            subpixel_positioning: true,
            preload_sizes: vec![],
        }
    }
}

/// Unified import settings for any resource type.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportSettings {
    Texture(TextureImportSettings),
    Audio(AudioImportSettings),
    Scene(SceneImportSettings),
    Mesh(MeshImportSettings),
    Font(FontImportSettings),
}

impl ImportSettings {
    /// Returns the resource type for these settings.
    pub fn resource_type(&self) -> ImportResourceType {
        match self {
            Self::Texture(_) => ImportResourceType::Texture,
            Self::Audio(_) => ImportResourceType::Audio,
            Self::Scene(_) => ImportResourceType::Scene,
            Self::Mesh(_) => ImportResourceType::Mesh,
            Self::Font(_) => ImportResourceType::Font,
        }
    }

    /// Creates default settings for the given resource type.
    /// Returns None for types without configurable settings (Shader, Script).
    pub fn defaults_for(resource_type: ImportResourceType) -> Option<Self> {
        match resource_type {
            ImportResourceType::Texture => Some(Self::Texture(TextureImportSettings::default())),
            ImportResourceType::Audio => Some(Self::Audio(AudioImportSettings::default())),
            ImportResourceType::Scene => Some(Self::Scene(SceneImportSettings::default())),
            ImportResourceType::Mesh => Some(Self::Mesh(MeshImportSettings::default())),
            ImportResourceType::Font => Some(Self::Font(FontImportSettings::default())),
            ImportResourceType::Shader | ImportResourceType::Script => None,
        }
    }
}

/// Per-file import override: maps a resource path to custom settings.
#[derive(Debug, Clone)]
pub struct ImportOverride {
    /// Resource path (e.g. "res://textures/hero.png").
    pub resource_path: String,
    /// Custom settings for this file.
    pub settings: ImportSettings,
}

/// The import settings panel state.
#[derive(Debug)]
pub struct ImportSettingsPanel {
    visible: bool,
    /// Currently selected resource path (if any).
    selected_path: Option<String>,
    /// Default import settings per resource type.
    defaults: HashMap<ImportResourceType, ImportSettings>,
    /// Per-file overrides.
    overrides: HashMap<String, ImportSettings>,
    /// Pending changes (not yet applied).
    pending: Option<PendingImportEdit>,
}

/// A pending edit to import settings.
#[derive(Debug, Clone)]
struct PendingImportEdit {
    resource_path: String,
    settings: ImportSettings,
}

impl ImportSettingsPanel {
    /// Creates a new panel with default settings for all types.
    pub fn new() -> Self {
        let mut defaults = HashMap::new();
        for rt in ImportResourceType::all() {
            if let Some(s) = ImportSettings::defaults_for(*rt) {
                defaults.insert(*rt, s);
            }
        }
        Self {
            visible: false,
            selected_path: None,
            defaults,
            overrides: HashMap::new(),
            pending: None,
        }
    }

    /// Opens the panel.
    pub fn open(&mut self) {
        self.visible = true;
    }

    /// Closes the panel and discards pending changes.
    pub fn close(&mut self) {
        self.visible = false;
        self.pending = None;
    }

    /// Whether the panel is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Selects a resource path for editing.
    pub fn select_resource(&mut self, path: &str) {
        self.selected_path = Some(path.to_string());
        self.pending = None;
    }

    /// Returns the currently selected resource path.
    pub fn selected_path(&self) -> Option<&str> {
        self.selected_path.as_deref()
    }

    /// Returns the inferred resource type for the selected path.
    pub fn selected_resource_type(&self) -> Option<ImportResourceType> {
        let path = self.selected_path.as_deref()?;
        let ext = path.rsplit('.').next()?;
        ImportResourceType::from_extension(ext)
    }

    /// Returns the effective settings for the selected resource
    /// (override > pending > default).
    pub fn effective_settings(&self) -> Option<&ImportSettings> {
        let path = self.selected_path.as_deref()?;
        // Check pending first
        if let Some(pending) = &self.pending {
            if pending.resource_path == path {
                return Some(&pending.settings);
            }
        }
        // Then check overrides
        if let Some(s) = self.overrides.get(path) {
            return Some(s);
        }
        // Fall back to defaults
        let rt = self.selected_resource_type()?;
        self.defaults.get(&rt)
    }

    /// Returns the default settings for a resource type.
    pub fn default_settings(&self, rt: ImportResourceType) -> Option<&ImportSettings> {
        self.defaults.get(&rt)
    }

    /// Updates the default settings for a resource type.
    pub fn set_default_settings(&mut self, rt: ImportResourceType, settings: ImportSettings) {
        self.defaults.insert(rt, settings);
    }

    /// Sets a pending edit for the selected resource.
    pub fn set_pending(&mut self, settings: ImportSettings) {
        if let Some(path) = &self.selected_path {
            self.pending = Some(PendingImportEdit {
                resource_path: path.clone(),
                settings,
            });
        }
    }

    /// Whether there are unsaved changes.
    pub fn has_pending_changes(&self) -> bool {
        self.pending.is_some()
    }

    /// Applies pending changes as a per-file override.
    pub fn apply(&mut self) -> bool {
        if let Some(pending) = self.pending.take() {
            self.overrides
                .insert(pending.resource_path, pending.settings);
            true
        } else {
            false
        }
    }

    /// Discards pending changes.
    pub fn discard(&mut self) {
        self.pending = None;
    }

    /// Removes the per-file override for a path (reverts to default).
    pub fn remove_override(&mut self, path: &str) -> bool {
        self.overrides.remove(path).is_some()
    }

    /// Returns whether a path has a per-file override.
    pub fn has_override(&self, path: &str) -> bool {
        self.overrides.contains_key(path)
    }

    /// Returns all per-file overrides.
    pub fn all_overrides(&self) -> &HashMap<String, ImportSettings> {
        &self.overrides
    }

    /// Returns the number of per-file overrides.
    pub fn override_count(&self) -> usize {
        self.overrides.len()
    }

    /// Resets the default settings for a resource type back to factory defaults.
    pub fn reset_defaults(&mut self, rt: ImportResourceType) {
        if let Some(s) = ImportSettings::defaults_for(rt) {
            self.defaults.insert(rt, s);
        }
    }

    /// Clears all per-file overrides.
    pub fn clear_all_overrides(&mut self) {
        self.overrides.clear();
    }

    /// Applies the same settings to multiple resource paths at once.
    pub fn batch_apply(&mut self, paths: &[&str], settings: &ImportSettings) -> usize {
        let mut count = 0;
        for path in paths {
            self.overrides.insert(path.to_string(), settings.clone());
            count += 1;
        }
        count
    }
}

impl Default for ImportSettingsPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ImportResourceType ----

    #[test]
    fn resource_type_roundtrip() {
        for rt in ImportResourceType::all() {
            let id = rt.id();
            let parsed = ImportResourceType::from_id(id).unwrap();
            assert_eq!(*rt, parsed);
        }
    }

    #[test]
    fn resource_type_from_extension() {
        assert_eq!(
            ImportResourceType::from_extension("png"),
            Some(ImportResourceType::Texture)
        );
        assert_eq!(
            ImportResourceType::from_extension("wav"),
            Some(ImportResourceType::Audio)
        );
        assert_eq!(
            ImportResourceType::from_extension("tscn"),
            Some(ImportResourceType::Scene)
        );
        assert_eq!(
            ImportResourceType::from_extension("ttf"),
            Some(ImportResourceType::Font)
        );
        assert_eq!(
            ImportResourceType::from_extension("gd"),
            Some(ImportResourceType::Script)
        );
        assert_eq!(ImportResourceType::from_extension("xyz"), None);
    }

    #[test]
    fn resource_type_extensions_nonempty() {
        for rt in ImportResourceType::all() {
            assert!(!rt.extensions().is_empty());
        }
    }

    #[test]
    fn resource_type_labels_nonempty() {
        for rt in ImportResourceType::all() {
            assert!(!rt.label().is_empty());
        }
    }

    // ---- Default settings ----

    #[test]
    fn defaults_for_configurable_types() {
        assert!(ImportSettings::defaults_for(ImportResourceType::Texture).is_some());
        assert!(ImportSettings::defaults_for(ImportResourceType::Audio).is_some());
        assert!(ImportSettings::defaults_for(ImportResourceType::Scene).is_some());
        assert!(ImportSettings::defaults_for(ImportResourceType::Mesh).is_some());
        assert!(ImportSettings::defaults_for(ImportResourceType::Font).is_some());
    }

    #[test]
    fn no_defaults_for_shader_script() {
        assert!(ImportSettings::defaults_for(ImportResourceType::Shader).is_none());
        assert!(ImportSettings::defaults_for(ImportResourceType::Script).is_none());
    }

    #[test]
    fn texture_defaults() {
        let s = TextureImportSettings::default();
        assert_eq!(s.compression, TextureCompression::VRAM);
        assert!(s.mipmaps);
        assert!(s.srgb);
        assert_eq!(s.max_size, 0);
    }

    #[test]
    fn audio_defaults() {
        let s = AudioImportSettings::default();
        assert_eq!(s.format, AudioFormatMode::Original);
        assert!(!s.loop_enabled);
        assert!(!s.force_mono);
    }

    #[test]
    fn scene_defaults() {
        let s = SceneImportSettings::default();
        assert!(s.import_animations);
        assert!(s.import_materials);
        assert_eq!(s.scale_factor, 1.0);
        assert_eq!(s.root_type, "Node3D");
    }

    #[test]
    fn mesh_defaults() {
        let s = MeshImportSettings::default();
        assert!(s.generate_lods);
        assert_eq!(s.lod_count, 3);
        assert!(!s.generate_collision);
    }

    #[test]
    fn font_defaults() {
        let s = FontImportSettings::default();
        assert_eq!(s.default_size, 16);
        assert!(s.antialiased);
        assert_eq!(s.hinting, FontHinting::Light);
    }

    // ---- ImportSettings::resource_type ----

    #[test]
    fn settings_resource_type() {
        let s = ImportSettings::Texture(TextureImportSettings::default());
        assert_eq!(s.resource_type(), ImportResourceType::Texture);
        let s = ImportSettings::Audio(AudioImportSettings::default());
        assert_eq!(s.resource_type(), ImportResourceType::Audio);
    }

    // ---- Panel basic ----

    #[test]
    fn panel_new_has_defaults() {
        let panel = ImportSettingsPanel::new();
        assert!(!panel.is_visible());
        assert!(panel.selected_path().is_none());
        // Should have defaults for 5 types (Texture, Audio, Scene, Mesh, Font)
        assert!(panel
            .default_settings(ImportResourceType::Texture)
            .is_some());
        assert!(panel.default_settings(ImportResourceType::Audio).is_some());
    }

    #[test]
    fn panel_open_close() {
        let mut panel = ImportSettingsPanel::new();
        panel.open();
        assert!(panel.is_visible());
        panel.close();
        assert!(!panel.is_visible());
    }

    #[test]
    fn select_resource_and_infer_type() {
        let mut panel = ImportSettingsPanel::new();
        panel.select_resource("res://textures/hero.png");
        assert_eq!(panel.selected_path(), Some("res://textures/hero.png"));
        assert_eq!(
            panel.selected_resource_type(),
            Some(ImportResourceType::Texture)
        );
    }

    #[test]
    fn effective_settings_returns_default() {
        let mut panel = ImportSettingsPanel::new();
        panel.select_resource("res://audio/music.ogg");
        let settings = panel.effective_settings().unwrap();
        assert!(matches!(settings, ImportSettings::Audio(_)));
    }

    // ---- Per-file overrides ----

    #[test]
    fn set_override_and_retrieve() {
        let mut panel = ImportSettingsPanel::new();
        let mut tex = TextureImportSettings::default();
        tex.compression = TextureCompression::Lossless;
        panel.select_resource("res://icon.png");
        panel.set_pending(ImportSettings::Texture(tex.clone()));
        assert!(panel.has_pending_changes());
        panel.apply();
        assert!(!panel.has_pending_changes());
        assert!(panel.has_override("res://icon.png"));

        // effective_settings should return override
        let s = panel.effective_settings().unwrap();
        if let ImportSettings::Texture(t) = s {
            assert_eq!(t.compression, TextureCompression::Lossless);
        } else {
            panic!("Expected texture settings");
        }
    }

    #[test]
    fn remove_override_reverts_to_default() {
        let mut panel = ImportSettingsPanel::new();
        let tex = TextureImportSettings {
            compression: TextureCompression::Uncompressed,
            ..Default::default()
        };
        panel.select_resource("res://tex.png");
        panel.set_pending(ImportSettings::Texture(tex));
        panel.apply();
        assert!(panel.has_override("res://tex.png"));

        panel.remove_override("res://tex.png");
        assert!(!panel.has_override("res://tex.png"));

        let s = panel.effective_settings().unwrap();
        if let ImportSettings::Texture(t) = s {
            assert_eq!(t.compression, TextureCompression::VRAM); // back to default
        } else {
            panic!("Expected texture settings");
        }
    }

    #[test]
    fn discard_pending() {
        let mut panel = ImportSettingsPanel::new();
        panel.select_resource("res://sound.wav");
        panel.set_pending(ImportSettings::Audio(AudioImportSettings {
            force_mono: true,
            ..Default::default()
        }));
        assert!(panel.has_pending_changes());
        panel.discard();
        assert!(!panel.has_pending_changes());
    }

    #[test]
    fn close_discards_pending() {
        let mut panel = ImportSettingsPanel::new();
        panel.open();
        panel.select_resource("res://a.png");
        panel.set_pending(ImportSettings::Texture(TextureImportSettings::default()));
        panel.close();
        assert!(!panel.has_pending_changes());
    }

    #[test]
    fn override_count() {
        let mut panel = ImportSettingsPanel::new();
        assert_eq!(panel.override_count(), 0);

        panel.select_resource("res://a.png");
        panel.set_pending(ImportSettings::Texture(TextureImportSettings::default()));
        panel.apply();

        panel.select_resource("res://b.wav");
        panel.set_pending(ImportSettings::Audio(AudioImportSettings::default()));
        panel.apply();

        assert_eq!(panel.override_count(), 2);
    }

    #[test]
    fn clear_all_overrides() {
        let mut panel = ImportSettingsPanel::new();
        panel.select_resource("res://a.png");
        panel.set_pending(ImportSettings::Texture(TextureImportSettings::default()));
        panel.apply();
        panel.clear_all_overrides();
        assert_eq!(panel.override_count(), 0);
    }

    // ---- Batch apply ----

    #[test]
    fn batch_apply_settings() {
        let mut panel = ImportSettingsPanel::new();
        let tex = ImportSettings::Texture(TextureImportSettings {
            compression: TextureCompression::Lossy,
            ..Default::default()
        });
        let count = panel.batch_apply(&["res://a.png", "res://b.png", "res://c.png"], &tex);
        assert_eq!(count, 3);
        assert_eq!(panel.override_count(), 3);
        assert!(panel.has_override("res://b.png"));
    }

    // ---- Update defaults ----

    #[test]
    fn update_default_settings() {
        let mut panel = ImportSettingsPanel::new();
        let tex = ImportSettings::Texture(TextureImportSettings {
            compression: TextureCompression::Lossless,
            ..Default::default()
        });
        panel.set_default_settings(ImportResourceType::Texture, tex);

        panel.select_resource("res://new_texture.png");
        let s = panel.effective_settings().unwrap();
        if let ImportSettings::Texture(t) = s {
            assert_eq!(t.compression, TextureCompression::Lossless);
        } else {
            panic!("Expected texture settings");
        }
    }

    #[test]
    fn reset_defaults_to_factory() {
        let mut panel = ImportSettingsPanel::new();
        panel.set_default_settings(
            ImportResourceType::Texture,
            ImportSettings::Texture(TextureImportSettings {
                compression: TextureCompression::Uncompressed,
                ..Default::default()
            }),
        );
        panel.reset_defaults(ImportResourceType::Texture);

        let s = panel.default_settings(ImportResourceType::Texture).unwrap();
        if let ImportSettings::Texture(t) = s {
            assert_eq!(t.compression, TextureCompression::VRAM);
        } else {
            panic!("Expected texture");
        }
    }

    // ---- No settings for non-configurable types ----

    #[test]
    fn no_effective_settings_for_script() {
        let mut panel = ImportSettingsPanel::new();
        panel.select_resource("res://main.gd");
        assert!(panel.effective_settings().is_none());
    }

    #[test]
    fn no_effective_settings_for_unknown() {
        let mut panel = ImportSettingsPanel::new();
        panel.select_resource("res://readme.txt");
        assert!(panel.effective_settings().is_none());
    }

    // ---- Pending prefers over override ----

    #[test]
    fn pending_takes_priority_over_override() {
        let mut panel = ImportSettingsPanel::new();
        // Set an override
        panel.select_resource("res://tex.png");
        panel.set_pending(ImportSettings::Texture(TextureImportSettings {
            compression: TextureCompression::Lossy,
            ..Default::default()
        }));
        panel.apply();

        // Now set a pending change
        panel.select_resource("res://tex.png");
        panel.set_pending(ImportSettings::Texture(TextureImportSettings {
            compression: TextureCompression::Lossless,
            ..Default::default()
        }));

        let s = panel.effective_settings().unwrap();
        if let ImportSettings::Texture(t) = s {
            assert_eq!(t.compression, TextureCompression::Lossless); // pending wins
        } else {
            panic!("Expected texture settings");
        }
    }

    // ---- Enum label coverage ----

    #[test]
    fn texture_compression_labels() {
        for c in TextureCompression::all() {
            assert!(!c.label().is_empty());
        }
    }

    #[test]
    fn audio_format_labels() {
        assert!(!AudioFormatMode::Original.label().is_empty());
        assert!(!AudioFormatMode::OggVorbis.label().is_empty());
        assert!(!AudioFormatMode::Wav.label().is_empty());
    }

    #[test]
    fn font_hinting_labels() {
        assert!(!FontHinting::None.label().is_empty());
        assert!(!FontHinting::Light.label().is_empty());
        assert!(!FontHinting::Normal.label().is_empty());
    }

    #[test]
    fn texture_filter_labels() {
        assert!(!TextureFilter::Nearest.label().is_empty());
        assert!(!TextureFilter::Linear.label().is_empty());
        assert!(!TextureFilter::NearestMipmap.label().is_empty());
        assert!(!TextureFilter::LinearMipmap.label().is_empty());
    }
}
