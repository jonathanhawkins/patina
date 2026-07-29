//! Resource import pipeline.
//!
//! Provides the [`ResourceImporter`] trait for pluggable import backends,
//! an [`ImportPipeline`] that orchestrates scanning and importing, and
//! built-in importers for `.tres` and `.tscn` files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gdcore::error::{EngineError, EngineResult};
use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Quaternion, Transform3D};
use gdresource::Resource;
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
    /// Keyframed tracks, in source order. Empty for formats whose animations
    /// carry no sampled channels.
    pub tracks: Vec<ImportedAnimationTrack>,
}

impl ImportedAnimation {
    /// Creates an animation with no tracks.
    pub fn new(name: impl Into<String>, duration: f64, loop_mode: AnimationLoopMode) -> Self {
        Self {
            name: name.into(),
            duration,
            loop_mode,
            tracks: Vec::new(),
        }
    }
}

/// One keyframed track of an [`ImportedAnimation`].
///
/// Mirrors a `tracks/<N>/*` block of an `[sub_resource type="Animation"]`
/// section, so the tscn writer can emit it verbatim and
/// `gdscene::animation_tscn` can read it back.
#[derive(Debug, Clone)]
pub struct ImportedAnimationTrack {
    /// Path of the animated node relative to the `AnimationPlayer`'s
    /// `root_node` (its parent — Godot's `AnimationMixer` default).
    pub node_path: String,
    /// Godot `.tscn` track type: `"position_3d"`, `"rotation_3d"` or
    /// `"scale_3d"`.
    pub track_type: String,
    /// Godot's `tracks/<N>/interp` selector: 0 = nearest, 1 = linear, 2 = cubic.
    pub interpolation: i64,
    /// Godot's flat keyframe layout: `(time, transition, x, y, z)` for
    /// position/scale tracks, `(time, transition, x, y, z, w)` for rotation
    /// (`Animation::_set` in upstream/godot/scene/resources/animation.cpp).
    pub keys: Vec<f32>,
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
        tracing::debug!(
            "Registered scene format importer: {}",
            importer.importer_name()
        );
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
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let importer = self.find_importer(ext).ok_or_else(|| {
            EngineError::NotFound(format!("no scene format importer for extension '{ext}'"))
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

// ---------------------------------------------------------------------------
// Node name suffixes — Godot's `_pre_fix_node` / `_post_fix_node` conventions
// ---------------------------------------------------------------------------

/// A node-name suffix recognised by the scene importer.
///
/// Godot lets a DCC tool drive import behaviour through the node's *name*:
/// `Body-col` becomes a mesh with a generated `StaticBody3D`, `Ground-colonly`
/// becomes collision with no visual mesh, `Crate-noimp` is dropped entirely
/// (upstream/godot/editor/import/3d/resource_importer_scene.cpp `_pre_fix_node`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeNameSuffix {
    /// No recognised suffix — the node imports as-is.
    None,
    /// `-noimp`: drop this node and its subtree.
    NoImport,
    /// `-col`: keep the mesh, add a trimesh `StaticBody3D`.
    Collision,
    /// `-convcol`: keep the mesh, add a convex `StaticBody3D`.
    ConvexCollision,
    /// `-colonly`: replace the mesh with a trimesh `StaticBody3D`.
    CollisionOnly,
    /// `-convcolonly`: replace the mesh with a convex `StaticBody3D`.
    ConvexCollisionOnly,
    /// `-rigid`: wrap the mesh in a `RigidBody3D`.
    Rigid,
    /// `-vehicle`: wrap the mesh in a `VehicleBody3D`.
    Vehicle,
    /// `-navmesh`: turn the mesh into a `NavigationRegion3D`.
    Navmesh,
    /// `-occ`: turn the mesh into an `OccluderInstance3D`.
    Occluder,
}

/// Godot's `_teststr`/`_fixstr` first strip trailing digits, spaces and
/// underscores, so Blender's `Body-col.001` → `Body-col_001` still matches.
fn trim_suffix_noise(name: &str) -> &str {
    let bytes = name.as_bytes();
    let mut end = bytes.len();
    // Every byte tested here is ASCII, and UTF-8 continuation bytes are >= 0x80,
    // so this can never cut a multi-byte character in half.
    while end > 0 {
        let c = bytes[end - 1];
        if c.is_ascii_digit() || c <= b' ' || c == b'_' {
            end -= 1;
        } else {
            break;
        }
    }
    &name[..end]
}

/// Port of Godot's `_teststr`: does `name` carry the `-<token>` / `_<token>` /
/// `$<token>` suffix marker?
fn teststr(name: &str, token: &str) -> bool {
    let what = trim_suffix_noise(name).to_ascii_lowercase();
    what.contains(&format!("${token}"))
        || what.ends_with(&format!("-{token}"))
        || what.ends_with(&format!("_{token}"))
}

/// Port of Godot's `_fixstr`: removes the suffix marker, preserving the trailing
/// digits/spaces that `_teststr` looked past.
fn fixstr(name: &str, token: &str) -> String {
    let what = trim_suffix_noise(name);
    let tail = &name[what.len()..];
    let lower = what.to_ascii_lowercase();

    let dollar = format!("${token}");
    if let Some(at) = lower.find(&dollar) {
        return format!("{}{}{}", &what[..at], &what[at + dollar.len()..], tail);
    }
    if lower.ends_with(&format!("-{token}")) || lower.ends_with(&format!("_{token}")) {
        let cut = what.len() - (token.len() + 1);
        return format!("{}{}", &what[..cut], tail);
    }
    what.to_string()
}

/// The suffix table, longest-match-first so `-colonly` is never read as `-col`
/// and `-convcol` is never read as `-col`.
const SUFFIX_TABLE: &[(&str, NodeNameSuffix)] = &[
    ("noimp", NodeNameSuffix::NoImport),
    ("convcolonly", NodeNameSuffix::ConvexCollisionOnly),
    ("colonly", NodeNameSuffix::CollisionOnly),
    ("convcol", NodeNameSuffix::ConvexCollision),
    ("col", NodeNameSuffix::Collision),
    ("navmesh", NodeNameSuffix::Navmesh),
    ("occ", NodeNameSuffix::Occluder),
    ("vehicle", NodeNameSuffix::Vehicle),
    ("rigid", NodeNameSuffix::Rigid),
];

/// Splits an imported node name into its base name and its import suffix.
///
/// `"Body-col"` → `("Body", NodeNameSuffix::Collision)`; a name with no
/// recognised suffix comes back unchanged.
pub fn split_name_suffix(name: &str) -> (String, NodeNameSuffix) {
    for (token, kind) in SUFFIX_TABLE {
        if teststr(name, token) {
            return (fixstr(name, token), *kind);
        }
    }
    (name.to_string(), NodeNameSuffix::None)
}

/// Godot's animation-name loop markers (`_post_fix_animations`): a clip called
/// `Walk-loop` imports as `Walk` with `LOOP_LINEAR`.
const ANIM_LOOP_TOKENS: &[&str] = &["loop_mode", "loop", "cycle"];

/// Applies the animation-name loop markers, returning the fixed name and mode.
fn split_animation_loop_suffix(name: &str) -> (String, AnimationLoopMode) {
    for token in ANIM_LOOP_TOKENS {
        if teststr(name, token) {
            return (fixstr(name, token), AnimationLoopMode::Linear);
        }
    }
    (name.to_string(), AnimationLoopMode::None)
}

// ---------------------------------------------------------------------------
// Shape payloads carried on generated CollisionShape3D nodes
// ---------------------------------------------------------------------------

/// Property key holding a generated shape's class name, set on the
/// `CollisionShape3D` nodes the suffix handling creates.
pub const SHAPE_TYPE_KEY: &str = "_shape_type";
/// Property key holding a generated shape's vertex payload.
pub const SHAPE_POINTS_KEY: &str = "_shape_points";

/// Builds the `CollisionShape3D` node for a generated shape.
///
/// `convex` selects Godot's two `_pre_gen_shape_list` outputs: a
/// `ConvexPolygonShape3D` over the primitive's unique points, or a
/// `ConcavePolygonShape3D` over its expanded triangle soup.
fn collision_shape_node(points: Vec<Vector3>, convex: bool) -> ImportedSceneNode {
    let class = if convex {
        "ConvexPolygonShape3D"
    } else {
        "ConcavePolygonShape3D"
    };
    ImportedSceneNode::new("CollisionShape3D", "CollisionShape3D")
        .with_property(SHAPE_TYPE_KEY, Variant::String(class.into()))
        .with_property(SHAPE_POINTS_KEY, Variant::PackedVector3Array(points))
}

// ---------------------------------------------------------------------------
// glTF → ImportedScene
// ---------------------------------------------------------------------------

/// Built-in glTF scene importer.
///
/// Parses the real document through [`gdresource::import_gltf_bytes`] and walks
/// its node graph into an [`ImportedScene`], applying Godot's node-name suffix
/// conventions and building an `AnimationPlayer` from the document's animation
/// channels. A file that is not glTF fails to parse rather than producing a
/// placeholder scene.
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
        opts.set("nodes/use_node_type_suffixes", Variant::Bool(true));
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
        let bytes = std::fs::read(path).map_err(EngineError::Io)?;
        // The parser is the gate: non-glTF bytes come back as a parse error
        // instead of a fabricated node tree.
        let doc = gdresource::import_gltf_bytes(&bytes, path)?;
        build_scene_from_gltf_document(&doc, path, options)
    }

    fn importer_name(&self) -> &str {
        "GltfSceneImporter"
    }
}

/// A glTF node, decoded from the document's `nodes` property.
struct GltfNode {
    name: String,
    mesh: Option<usize>,
    camera: Option<usize>,
    light: Option<usize>,
    children: Vec<usize>,
    transform: Transform3D,
}

fn opt_index(dict: &gdvariant::Dictionary, key: &str) -> Option<usize> {
    match dict.get(key) {
        Some(Variant::Int(i)) if *i >= 0 => Some(*i as usize),
        _ => None,
    }
}

fn floats(dict: &gdvariant::Dictionary, key: &str) -> Vec<f32> {
    match dict.get(key) {
        Some(Variant::Array(items)) => items
            .iter()
            .map(|v| match v {
                Variant::Float(f) => *f as f32,
                Variant::Int(i) => *i as f32,
                _ => 0.0,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn decode_gltf_nodes(doc: &Resource) -> Vec<GltfNode> {
    let Some(Variant::Array(entries)) = doc.get_property("nodes") else {
        return Vec::new();
    };
    entries
        .iter()
        .map(|entry| {
            let Variant::Dictionary(d) = entry else {
                return GltfNode {
                    name: "Node".into(),
                    mesh: None,
                    camera: None,
                    light: None,
                    children: Vec::new(),
                    transform: Transform3D::IDENTITY,
                };
            };
            let name = match d.get("name") {
                Some(Variant::String(s)) if !s.is_empty() => s.clone(),
                _ => "Node".to_string(),
            };
            let children = match d.get("children") {
                Some(Variant::Array(kids)) => kids
                    .iter()
                    .filter_map(|k| match k {
                        Variant::Int(i) if *i >= 0 => Some(*i as usize),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let t = floats(d, "translation");
            let r = floats(d, "rotation");
            let s = floats(d, "scale");
            let rotation = Quaternion::new(
                r.first().copied().unwrap_or(0.0),
                r.get(1).copied().unwrap_or(0.0),
                r.get(2).copied().unwrap_or(0.0),
                r.get(3).copied().unwrap_or(1.0),
            );
            let scale = Vector3::new(
                s.first().copied().unwrap_or(1.0),
                s.get(1).copied().unwrap_or(1.0),
                s.get(2).copied().unwrap_or(1.0),
            );
            let origin = Vector3::new(
                t.first().copied().unwrap_or(0.0),
                t.get(1).copied().unwrap_or(0.0),
                t.get(2).copied().unwrap_or(0.0),
            );
            GltfNode {
                name,
                mesh: opt_index(d, "mesh"),
                camera: opt_index(d, "camera"),
                light: opt_index(d, "light"),
                children,
                transform: Transform3D {
                    basis: Basis::from_quaternion_scale(rotation, scale),
                    origin,
                },
            }
        })
        .collect()
}

/// The `Light3D` subclass a `KHR_lights_punctual` entry maps to.
fn light_class(doc: &Resource, index: usize) -> &'static str {
    let Some(Variant::Array(lights)) = doc.get_property("lights") else {
        return "OmniLight3D";
    };
    let Some(Variant::Dictionary(d)) = lights.get(index) else {
        return "OmniLight3D";
    };
    match d.get("type") {
        Some(Variant::String(s)) if s == "directional" => "DirectionalLight3D",
        Some(Variant::String(s)) if s == "spot" => "SpotLight3D",
        _ => "OmniLight3D",
    }
}

/// Reads the primitive subresources belonging to glTF mesh `mesh_index`, as
/// `(vertices, indices)` pairs.
fn mesh_primitives(doc: &Resource, mesh_index: usize) -> Vec<(Vec<Vector3>, Vec<usize>)> {
    let prefix = format!("mesh_{mesh_index}_prim_");
    let mut keys: Vec<&String> = doc
        .subresources
        .keys()
        .filter(|k| k.starts_with(prefix.as_str()))
        .collect();
    keys.sort();

    keys.into_iter()
        .filter_map(|key| {
            let sub = doc.subresources.get(key)?;
            let vertices = match sub.get_property("vertices") {
                Some(Variant::Array(items)) => items
                    .iter()
                    .filter_map(|v| match v {
                        Variant::Vector3(p) => Some(*p),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            let indices = match sub.get_property("indices") {
                Some(Variant::Array(items)) => items
                    .iter()
                    .filter_map(|v| match v {
                        Variant::Int(i) if *i >= 0 => Some(*i as usize),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            Some((vertices, indices))
        })
        .collect()
}

/// Godot's `_pre_gen_shape_list`: a convex hull's point cloud, or the expanded
/// triangle soup a `ConcavePolygonShape3D` stores.
fn shape_points(doc: &Resource, mesh_index: usize, convex: bool) -> Vec<Vector3> {
    let mut out = Vec::new();
    for (vertices, indices) in mesh_primitives(doc, mesh_index) {
        if convex {
            out.extend(vertices);
        } else {
            for &i in &indices {
                if let Some(v) = vertices.get(i) {
                    out.push(*v);
                }
            }
        }
    }
    out
}

/// Walk state shared by [`convert_gltf_node`].
struct GltfWalk<'a> {
    doc: &'a Resource,
    nodes: &'a [GltfNode],
    use_suffixes: bool,
    /// glTF node index → path of the imported node relative to the scene root,
    /// so animation channels can be retargeted. Absent for dropped nodes.
    paths: HashMap<usize, String>,
    warnings: Vec<String>,
}

/// Converts one glTF node (and its subtree) into an [`ImportedSceneNode`].
///
/// `parent_path` is the scene-root-relative path of the parent, used to record
/// this node's own path for animation retargeting. Returns `None` when the node
/// carries the `-noimp` suffix or its name is empty once the suffix is removed
/// (Godot skips both).
fn convert_gltf_node(
    walk: &mut GltfWalk,
    index: usize,
    parent_path: &str,
    visited: &mut Vec<usize>,
) -> Option<ImportedSceneNode> {
    if visited.contains(&index) {
        walk.warnings
            .push(format!("gltf: node {index} is part of a cycle; skipped"));
        return None;
    }
    // Copied out up front so the walk state can be mutated while building.
    let doc: &Resource = walk.doc;
    let (raw_name, mesh, camera, light, child_indices, transform) = {
        let node = walk.nodes.get(index)?;
        (
            node.name.clone(),
            node.mesh,
            node.camera,
            node.light,
            node.children.clone(),
            node.transform,
        )
    };

    let (base_name, suffix) = if walk.use_suffixes {
        split_name_suffix(&raw_name)
    } else {
        (raw_name.clone(), NodeNameSuffix::None)
    };
    if suffix == NodeNameSuffix::NoImport {
        return None;
    }
    if base_name.is_empty() {
        walk.warnings.push(format!(
            "gltf: skipped node '{raw_name}' — empty name after removing its suffix"
        ));
        return None;
    }

    // The suffix conventions only apply to nodes that actually carry geometry;
    // a bare Node3D named `-col` has no mesh to build a shape from.
    let has_mesh = mesh.is_some();
    let effective = if has_mesh {
        suffix
    } else {
        NodeNameSuffix::None
    };

    let visual_class = if has_mesh {
        "MeshInstance3D"
    } else if camera.is_some() {
        "Camera3D"
    } else if let Some(light) = light {
        light_class(doc, light)
    } else {
        "Node3D"
    };

    let node_path = if parent_path.is_empty() {
        base_name.clone()
    } else {
        format!("{parent_path}/{base_name}")
    };

    // Build this node's own shell first — the suffix decides whether the mesh
    // survives, is wrapped, or is replaced by a body.
    let mut out = match effective {
        NodeNameSuffix::CollisionOnly | NodeNameSuffix::ConvexCollisionOnly => {
            let convex = effective == NodeNameSuffix::ConvexCollisionOnly;
            let points = shape_points(doc, mesh.unwrap(), convex);
            ImportedSceneNode::new(&base_name, "StaticBody3D")
                .with_child(collision_shape_node(points, convex))
        }
        NodeNameSuffix::Rigid | NodeNameSuffix::Vehicle => {
            let class = if effective == NodeNameSuffix::Rigid {
                "RigidBody3D"
            } else {
                "VehicleBody3D"
            };
            let points = shape_points(doc, mesh.unwrap(), true);
            let mut mesh_child = ImportedSceneNode::new(&base_name, "MeshInstance3D");
            attach_mesh(&mut mesh_child, mesh.unwrap());
            ImportedSceneNode::new(&base_name, class)
                .with_child(mesh_child)
                .with_child(collision_shape_node(points, true))
        }
        NodeNameSuffix::Navmesh => ImportedSceneNode::new(&base_name, "NavigationRegion3D"),
        NodeNameSuffix::Occluder => ImportedSceneNode::new(&base_name, "OccluderInstance3D"),
        _ => {
            let mut n = ImportedSceneNode::new(&base_name, visual_class);
            if let Some(mesh) = mesh {
                attach_mesh(&mut n, mesh);
            }
            if let Some(camera) = camera {
                n.properties
                    .insert("_gltf_camera".into(), Variant::Int(camera as i64));
            }
            if let Some(light) = light {
                n.properties
                    .insert("_gltf_light".into(), Variant::Int(light as i64));
            }
            if matches!(
                effective,
                NodeNameSuffix::Collision | NodeNameSuffix::ConvexCollision
            ) {
                // Godot keeps the visual mesh and hangs a generated StaticBody3D
                // off it (`_add_shapes` on the body).
                let convex = effective == NodeNameSuffix::ConvexCollision;
                let points = shape_points(doc, mesh.unwrap(), convex);
                n.children.push(
                    ImportedSceneNode::new("StaticBody3D", "StaticBody3D")
                        .with_child(collision_shape_node(points, convex)),
                );
            }
            n
        }
    };

    out.properties
        .insert("transform".into(), Variant::Transform3D(transform));
    walk.paths.insert(index, node_path.clone());

    visited.push(index);
    for child in child_indices {
        if let Some(c) = convert_gltf_node(walk, child, &node_path, visited) {
            out.children.push(c);
        }
    }
    visited.pop();

    Some(out)
}

/// Records the glTF mesh a `MeshInstance3D` draws. The tscn writer turns this
/// into the `[sub_resource type="ArrayMesh"]` the node's `mesh` points at.
fn attach_mesh(node: &mut ImportedSceneNode, mesh_index: usize) {
    node.properties
        .insert("_gltf_mesh".into(), Variant::Int(mesh_index as i64));
}

/// Converts the document's animation channels into [`ImportedAnimation`]s,
/// retargeting each channel onto the imported node's scene path.
fn build_gltf_animations(doc: &Resource, paths: &HashMap<usize, String>) -> Vec<ImportedAnimation> {
    let Some(Variant::Array(clips)) = doc.get_property("animations") else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for clip in clips {
        let Variant::Dictionary(clip) = clip else {
            continue;
        };
        let raw_name = match clip.get("name") {
            Some(Variant::String(s)) if !s.is_empty() => s.clone(),
            _ => "Animation".to_string(),
        };
        let (name, loop_mode) = split_animation_loop_suffix(&raw_name);
        let duration = match clip.get("length") {
            Some(Variant::Float(f)) => *f,
            Some(Variant::Int(i)) => *i as f64,
            _ => 0.0,
        };

        let mut tracks = Vec::new();
        if let Some(Variant::Array(channels)) = clip.get("channels") {
            for channel in channels {
                let Variant::Dictionary(ch) = channel else {
                    continue;
                };
                let Some(target) = opt_index(ch, "target_node") else {
                    continue;
                };
                // A channel aimed at a `-noimp` node has nothing left to drive.
                let Some(node_path) = paths.get(&target) else {
                    continue;
                };
                let (track_type, components) = match ch.get("path") {
                    Some(Variant::String(p)) if p == "translation" => ("position_3d", 3usize),
                    Some(Variant::String(p)) if p == "rotation" => ("rotation_3d", 4),
                    Some(Variant::String(p)) if p == "scale" => ("scale_3d", 3),
                    // Morph-target weights need per-target track names Patina
                    // does not model yet.
                    _ => continue,
                };
                let interpolation = match ch.get("interpolation") {
                    Some(Variant::String(i)) if i == "step" => 0,
                    Some(Variant::String(i)) if i == "cubicspline" => 2,
                    _ => 1,
                };
                let times = floats(ch, "times");
                let values = floats(ch, "values");

                let mut keys = Vec::with_capacity(times.len() * (components + 2));
                for (k, time) in times.iter().enumerate() {
                    let start = k * components;
                    let Some(slice) = values.get(start..start + components) else {
                        break;
                    };
                    keys.push(*time);
                    // Godot's per-key easing exponent; the editor always writes 1.
                    keys.push(1.0);
                    keys.extend_from_slice(slice);
                }
                if keys.is_empty() {
                    continue;
                }
                tracks.push(ImportedAnimationTrack {
                    node_path: node_path.clone(),
                    track_type: track_type.to_string(),
                    interpolation,
                    keys,
                });
            }
        }

        out.push(ImportedAnimation {
            name,
            duration,
            loop_mode,
            tracks,
        });
    }
    out
}

/// Builds an [`ImportedScene`] from a parsed glTF document.
///
/// The scene root is a `Node3D` named after the source file (Godot's default
/// root for a multi-root document); the document's scene roots become its
/// children.
fn build_scene_from_gltf_document(
    doc: &Resource,
    path: &Path,
    options: &SceneImportOptions,
) -> EngineResult<ImportedScene> {
    let root_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Scene")
        .to_string();

    let nodes = decode_gltf_nodes(doc);

    // Scene roots: the document's declared list, else every node no other node
    // claims as a child.
    let declared: Vec<usize> = match doc.get_property("scene_roots") {
        Some(Variant::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Variant::Int(i) if *i >= 0 => Some(*i as usize),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let roots: Vec<usize> = if declared.is_empty() {
        let mut is_child = vec![false; nodes.len()];
        for node in &nodes {
            for &c in &node.children {
                if let Some(slot) = is_child.get_mut(c) {
                    *slot = true;
                }
            }
        }
        (0..nodes.len()).filter(|i| !is_child[*i]).collect()
    } else {
        declared
    };

    let use_suffixes = options
        .get("nodes/use_node_type_suffixes")
        .and_then(|v| match v {
            Variant::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(true);

    let mut walk = GltfWalk {
        doc,
        nodes: &nodes,
        use_suffixes,
        paths: HashMap::new(),
        warnings: Vec::new(),
    };

    let mut root = ImportedSceneNode::new(&root_name, "Node3D");
    let mut visited = Vec::new();
    for index in roots {
        if let Some(child) = convert_gltf_node(&mut walk, index, "", &mut visited) {
            root.children.push(child);
        }
    }

    let import_anims = options
        .get("animation/import")
        .and_then(|v| match v {
            Variant::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(true);

    let animations = if import_anims {
        build_gltf_animations(doc, &walk.paths)
    } else {
        Vec::new()
    };
    if !animations.is_empty() {
        root.children
            .push(ImportedSceneNode::new("AnimationPlayer", "AnimationPlayer"));
    }

    // Godot counts one imported mesh per *primitive* — a glTF mesh with two
    // primitives becomes two surfaces.
    let mesh_count = match doc.get_property("primitive_count") {
        Some(Variant::Int(n)) if *n >= 0 => *n as usize,
        _ => 0,
    };

    Ok(ImportedScene {
        root,
        animations,
        mesh_count,
        warnings: std::mem::take(&mut walk.warnings),
    })
}

/// Built-in OBJ (Wavefront) scene importer.
///
/// Parses the real `v`/`f`/`o`/`g` records: each object/group becomes a
/// `MeshInstance3D` and the node-name suffixes apply to the group names, so
/// `o Ground-colonly` imports as collision with no visual mesh.
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
        let text = std::fs::read_to_string(path).map_err(EngineError::Io)?;
        let root_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("ObjScene")
            .to_string();
        build_scene_from_obj(&text, &root_name)
    }

    fn importer_name(&self) -> &str {
        "ObjSceneImporter"
    }
}

/// A `v`/`f` group collected from an OBJ file.
struct ObjGroup {
    name: String,
    vertices: Vec<Vector3>,
    faces: usize,
}

/// Parses OBJ text into an [`ImportedScene`].
///
/// Only the geometry records Patina models are read (`v` positions, `f` faces,
/// `o`/`g` grouping); material libraries and free-form surfaces are ignored.
fn build_scene_from_obj(text: &str, root_name: &str) -> EngineResult<ImportedScene> {
    let mut all_positions: Vec<Vector3> = Vec::new();
    let mut groups: Vec<ObjGroup> = Vec::new();
    let mut current: Option<ObjGroup> = None;

    for line in text.lines() {
        let line = line.trim();
        let Some((tag, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        match tag {
            "v" => {
                let n: Vec<f32> = rest
                    .split_whitespace()
                    .filter_map(|t| t.parse::<f32>().ok())
                    .collect();
                if n.len() >= 3 {
                    all_positions.push(Vector3::new(n[0], n[1], n[2]));
                }
            }
            "o" | "g" => {
                if let Some(group) = current.take() {
                    groups.push(group);
                }
                current = Some(ObjGroup {
                    name: rest.trim().to_string(),
                    vertices: Vec::new(),
                    faces: 0,
                });
            }
            "f" => {
                let group = current.get_or_insert_with(|| ObjGroup {
                    name: root_name.to_string(),
                    vertices: Vec::new(),
                    faces: 0,
                });
                group.faces += 1;
                // `f v/vt/vn` — the position index is the first component, and
                // OBJ indices are 1-based (negative indices count from the end).
                for token in rest.split_whitespace() {
                    let Ok(raw) = token.split('/').next().unwrap_or("").parse::<i64>() else {
                        continue;
                    };
                    let idx = if raw > 0 {
                        (raw - 1) as usize
                    } else if raw < 0 {
                        match all_positions.len().checked_sub((-raw) as usize) {
                            Some(i) => i,
                            None => continue,
                        }
                    } else {
                        continue;
                    };
                    if let Some(v) = all_positions.get(idx) {
                        group.vertices.push(*v);
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(group) = current.take() {
        groups.push(group);
    }

    if all_positions.is_empty() {
        return Err(EngineError::Parse(format!(
            "obj: no vertex positions in '{root_name}'"
        )));
    }

    let mut root = ImportedSceneNode::new(root_name, "Node3D");
    let mut mesh_count = 0usize;
    for group in &groups {
        if group.faces == 0 {
            continue;
        }
        mesh_count += 1;
        let (base_name, suffix) = split_name_suffix(&group.name);
        if suffix == NodeNameSuffix::NoImport || base_name.is_empty() {
            continue;
        }
        let node = match suffix {
            NodeNameSuffix::CollisionOnly | NodeNameSuffix::ConvexCollisionOnly => {
                let convex = suffix == NodeNameSuffix::ConvexCollisionOnly;
                ImportedSceneNode::new(&base_name, "StaticBody3D")
                    .with_child(collision_shape_node(group.vertices.clone(), convex))
            }
            NodeNameSuffix::Collision | NodeNameSuffix::ConvexCollision => {
                let convex = suffix == NodeNameSuffix::ConvexCollision;
                ImportedSceneNode::new(&base_name, "MeshInstance3D").with_child(
                    ImportedSceneNode::new("StaticBody3D", "StaticBody3D")
                        .with_child(collision_shape_node(group.vertices.clone(), convex)),
                )
            }
            _ => ImportedSceneNode::new(&base_name, "MeshInstance3D"),
        };
        root.children.push(node);
    }

    Ok(ImportedScene {
        root,
        animations: Vec::new(),
        mesh_count,
        warnings: Vec::new(),
    })
}

/// Built-in FBX scene importer.
///
/// FBX parsing is a vendoring decision that has not been made — `THIRDPARTY_STRATEGY.md`
/// classifies it as "wrap via FFI / ufbx", and ufbx is not vendored anywhere in
/// the workspace. Rather than fabricate a node tree from bytes it cannot read,
/// this importer reports the format as unsupported.
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
        _options: &SceneImportOptions,
    ) -> EngineResult<ImportedScene> {
        if !path.exists() {
            return Err(EngineError::NotFound(format!(
                "file not found: {}",
                path.display()
            )));
        }
        Err(EngineError::Unsupported(format!(
            "fbx: no FBX parser is vendored, cannot import '{}'",
            path.display()
        )))
    }

    fn importer_name(&self) -> &str {
        "FbxSceneImporter"
    }
}

// ---------------------------------------------------------------------------
// ImportedScene → PackedScene (`.scn`)
// ---------------------------------------------------------------------------

/// A `[sub_resource]` section accumulated while writing a scene.
struct SubResource {
    id: String,
    class: String,
    lines: Vec<String>,
}

/// Serialises an [`ImportedScene`] into Godot `.tscn` text.
///
/// The result parses back through `gdscene::PackedScene::from_tscn`: generated
/// shapes and imported meshes become `[sub_resource]` sections, and an
/// `AnimationPlayer` node picks up an `AnimationLibrary` built from
/// [`ImportedScene::animations`].
pub fn imported_scene_to_tscn(scene: &ImportedScene) -> String {
    let mut subs: Vec<SubResource> = Vec::new();
    let mut nodes: Vec<String> = Vec::new();

    // Animations first, so the library id exists before the player node needs it.
    let library_id = (!scene.animations.is_empty()).then(|| {
        let mut entries: Vec<String> = Vec::new();
        for (i, anim) in scene.animations.iter().enumerate() {
            let id = format!("Animation_{i}");
            let mut lines = vec![
                format!("resource_name = \"{}\"", escape_tscn(&anim.name)),
                format!("length = {}", format_float(anim.duration as f32)),
            ];
            let loop_mode = match anim.loop_mode {
                AnimationLoopMode::None => 0,
                AnimationLoopMode::Linear => 1,
                AnimationLoopMode::PingPong => 2,
            };
            lines.push(format!("loop_mode = {loop_mode}"));
            for (t, track) in anim.tracks.iter().enumerate() {
                lines.push(format!("tracks/{t}/type = \"{}\"", track.track_type));
                lines.push(format!("tracks/{t}/enabled = true"));
                lines.push(format!(
                    "tracks/{t}/path = NodePath(\"{}\")",
                    escape_tscn(&track.node_path)
                ));
                lines.push(format!("tracks/{t}/interp = {}", track.interpolation));
                lines.push(format!(
                    "tracks/{t}/keys = PackedFloat32Array({})",
                    track
                        .keys
                        .iter()
                        .map(|k| format_float(*k))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            subs.push(SubResource {
                id: id.clone(),
                class: "Animation".into(),
                lines,
            });
            entries.push(format!(
                "\"{}\": SubResource(\"{id}\")",
                escape_tscn(&anim.name)
            ));
        }
        let library_id = "AnimationLibrary_0".to_string();
        subs.push(SubResource {
            id: library_id.clone(),
            class: "AnimationLibrary".into(),
            lines: vec![format!("_data = {{{}}}", entries.join(", "))],
        });
        library_id
    });

    write_node(
        &scene.root,
        None,
        &mut nodes,
        &mut subs,
        library_id.as_deref(),
    );

    let mut out = format!(
        "[gd_scene load_steps={} format=3]\n",
        subs.len().saturating_add(1)
    );
    for sub in &subs {
        out.push_str(&format!(
            "\n[sub_resource type=\"{}\" id=\"{}\"]\n",
            sub.class, sub.id
        ));
        for line in &sub.lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    for node in &nodes {
        out.push('\n');
        out.push_str(node);
    }
    out
}

/// Emits one node section and recurses into its children.
///
/// `parent` is the scene-relative path a `.tscn` `parent=` attribute needs;
/// `None` marks the scene root, which carries no attribute.
fn write_node(
    node: &ImportedSceneNode,
    parent: Option<&str>,
    out: &mut Vec<String>,
    subs: &mut Vec<SubResource>,
    library_id: Option<&str>,
) {
    let mut section = match parent {
        None => format!(
            "[node name=\"{}\" type=\"{}\"]\n",
            escape_tscn(&node.name),
            node.node_type
        ),
        Some(p) => format!(
            "[node name=\"{}\" type=\"{}\" parent=\"{}\"]\n",
            escape_tscn(&node.name),
            node.node_type,
            escape_tscn(p)
        ),
    };

    // A generated CollisionShape3D carries its shape inline; promote it to a
    // `[sub_resource]` the node points at.
    if let (Some(Variant::String(class)), Some(Variant::PackedVector3Array(points))) = (
        node.properties.get(SHAPE_TYPE_KEY),
        node.properties.get(SHAPE_POINTS_KEY),
    ) {
        let id = format!("{class}_{}", subs.len());
        let key = if class == "ConvexPolygonShape3D" {
            "points"
        } else {
            "data"
        };
        subs.push(SubResource {
            id: id.clone(),
            class: class.clone(),
            lines: vec![format!("{key} = {}", format_packed_vector3(points))],
        });
        section.push_str(&format!("shape = SubResource(\"{id}\")\n"));
    }

    if node.node_type == "AnimationPlayer" {
        if let Some(library_id) = library_id {
            section.push_str(&format!(
                "libraries = {{\"\": SubResource(\"{library_id}\")}}\n"
            ));
        }
    }

    // Remaining properties, sorted so the same scene always writes byte-identically.
    let mut keys: Vec<&String> = node
        .properties
        .keys()
        .filter(|k| k.as_str() != SHAPE_TYPE_KEY && k.as_str() != SHAPE_POINTS_KEY)
        .collect();
    keys.sort();
    for key in keys {
        let value = &node.properties[key];
        section.push_str(&format!(
            "{key} = {}\n",
            gdscene::scene_saver::format_variant_value(value)
        ));
    }

    out.push(section);

    let self_path = match parent {
        None => ".".to_string(),
        Some(".") => node.name.clone(),
        Some(p) => format!("{p}/{}", node.name),
    };
    for child in &node.children {
        write_node(child, Some(&self_path), out, subs, library_id);
    }
}

fn escape_tscn(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn format_float(f: f32) -> String {
    if f == f.trunc() && f.is_finite() {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

fn format_packed_vector3(points: &[Vector3]) -> String {
    let items: Vec<String> = points
        .iter()
        .flat_map(|p| [format_float(p.x), format_float(p.y), format_float(p.z)])
        .collect();
    format!("PackedVector3Array({})", items.join(", "))
}

/// Returns a registry with the built-in 3D scene format importers registered.
pub fn default_scene_importer_registry() -> SceneFormatImporterRegistry {
    let mut reg = SceneFormatImporterRegistry::new();
    reg.register_importer(Box::new(GltfSceneImporter));
    reg.register_importer(Box::new(ObjSceneImporter));
    reg.register_importer(Box::new(FbxSceneImporter));
    reg
}

/// Imports a 3D source asset into a `PackedScene` file under
/// `<project_root>/.godot/imported/`, returning the path written.
///
/// This is the wiring Godot's `ResourceImporterScene::import` performs: run the
/// format importer, apply the post-import hooks, then save the result as a
/// `.scn` the rest of the engine loads like any other scene.
pub fn import_scene_to_packed_scene(
    registry: &SceneFormatImporterRegistry,
    source_path: &Path,
    project_root: &Path,
) -> EngineResult<PathBuf> {
    let scene = registry.import_scene(source_path, None)?;
    let text = imported_scene_to_tscn(&scene);

    let stem = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("scene");
    // Godot keys the cached file on a digest of the source path so two assets
    // with the same basename cannot collide.
    let digest = hash_bytes(source_path.to_string_lossy().as_bytes());
    let dir = project_root.join(".godot").join("imported");
    std::fs::create_dir_all(&dir).map_err(EngineError::Io)?;
    let out = dir.join(format!("{stem}-{digest:016x}.scn"));
    std::fs::write(&out, text).map_err(EngineError::Io)?;
    Ok(out)
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

    /// A buffer-free glTF document: enough to exercise the node walk without a
    /// binary blob. `Gizmo-noimp` must be dropped and `Torso` kept.
    const NODES_ONLY_GLTF: &str = r#"{
      "asset": {"version": "2.0"},
      "scene": 0,
      "scenes": [{"nodes": [0]}],
      "nodes": [
        {"name": "Torso", "children": [1], "translation": [1.0, 2.0, 3.0]},
        {"name": "Gizmo-noimp"}
      ]
    }"#;

    #[test]
    fn gltf_import_reads_real_node_names() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("model.gltf");
        std::fs::write(&path, NODES_ONLY_GLTF).unwrap();

        let reg = make_scene_registry();
        let scene = reg.import_scene(&path, None).unwrap();
        assert_eq!(scene.root.name, "model");
        assert_eq!(scene.root.node_type, "Node3D");
        // The document's own name, not a `<stem>_Mesh` invention.
        assert_eq!(scene.root.children.len(), 1);
        assert_eq!(scene.root.children[0].name, "Torso");
        assert_eq!(scene.root.children[0].node_type, "Node3D");
        // `-noimp` dropped the only child.
        assert!(scene.root.children[0].children.is_empty());
        // No meshes in the document means no meshes in the scene.
        assert_eq!(scene.mesh_count, 0);
        assert!(scene.animations.is_empty());
    }

    #[test]
    fn gltf_import_preserves_node_transform() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("model.gltf");
        std::fs::write(&path, NODES_ONLY_GLTF).unwrap();

        let reg = make_scene_registry();
        let scene = reg.import_scene(&path, None).unwrap();
        match scene.root.children[0].properties.get("transform") {
            Some(Variant::Transform3D(t)) => {
                assert_eq!(t.origin, Vector3::new(1.0, 2.0, 3.0));
            }
            other => panic!("expected the glTF translation on the node, got {other:?}"),
        }
    }

    #[test]
    fn gltf_import_rejects_non_gltf_bytes() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("model.gltf");
        // The old importer turned any file over 100 bytes into a one-mesh scene
        // with a `"default"` animation.
        std::fs::write(&path, "x".repeat(200)).unwrap();

        let reg = make_scene_registry();
        assert!(reg.import_scene(&path, None).is_err());
    }

    #[test]
    fn gltf_import_no_animations() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("static.gltf");
        std::fs::write(&path, NODES_ONLY_GLTF).unwrap();

        let mut opts = SceneImportOptions::new();
        opts.set("animation/import", Variant::Bool(false));

        let reg = make_scene_registry();
        let scene = reg.import_scene(&path, Some(&opts)).unwrap();
        assert!(scene.animations.is_empty());
        assert!(
            !scene
                .root
                .children
                .iter()
                .any(|c| c.node_type == "AnimationPlayer"),
            "no clips means no AnimationPlayer node"
        );
    }

    #[test]
    fn obj_import_scene() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cube.obj");
        std::fs::write(&path, "o Face\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();

        let reg = make_scene_registry();
        let scene = reg.import_scene(&path, None).unwrap();
        assert_eq!(scene.root.name, "cube");
        assert!(scene.animations.is_empty());
        assert_eq!(scene.mesh_count, 1);
        assert_eq!(scene.root.node_count(), 2); // root + the `Face` group
        assert_eq!(scene.root.children[0].name, "Face");
    }

    #[test]
    fn obj_import_applies_colonly_suffix() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("level.obj");
        std::fs::write(
            &path,
            "o Ground-colonly\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
        )
        .unwrap();

        let reg = make_scene_registry();
        let scene = reg.import_scene(&path, None).unwrap();
        let ground = &scene.root.children[0];
        assert_eq!(ground.name, "Ground");
        assert_eq!(ground.node_type, "StaticBody3D");
        assert_eq!(ground.children[0].node_type, "CollisionShape3D");
    }

    #[test]
    fn obj_import_rejects_a_file_with_no_geometry() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("empty.obj");
        std::fs::write(&path, "# just a comment\n").unwrap();

        let reg = make_scene_registry();
        assert!(reg.import_scene(&path, None).is_err());
    }

    #[test]
    fn fbx_import_reports_unsupported_rather_than_fabricating() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("character.fbx");
        std::fs::write(&path, "FBX binary data stub").unwrap();

        let reg = make_scene_registry();
        // No FBX parser is vendored — the old importer invented a
        // MeshInstance3D + Skeleton3D + an "idle" clip from these 20 bytes.
        assert!(matches!(
            reg.import_scene(&path, None),
            Err(EngineError::Unsupported(_))
        ));
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
        assert_eq!(
            opts.get("meshes/ensure_tangents"),
            Some(&Variant::Bool(true))
        );
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
            scene
                .root
                .properties
                .insert("_imported".into(), Variant::Bool(true));
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
        std::fs::write(&path, NODES_ONLY_GLTF).unwrap();

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

    // ---------------------------------------------------------------
    // ImportedScene -> .tscn
    // ---------------------------------------------------------------

    #[test]
    fn tscn_writer_promotes_a_shape_to_a_sub_resource() {
        let scene = ImportedScene {
            root: ImportedSceneNode::new("Level", "Node3D").with_child(
                ImportedSceneNode::new("Ground", "StaticBody3D").with_child(collision_shape_node(
                    vec![
                        Vector3::new(0.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                    ],
                    false,
                )),
            ),
            animations: Vec::new(),
            mesh_count: 1,
            warnings: Vec::new(),
        };

        let text = imported_scene_to_tscn(&scene);
        assert!(
            text.contains("[sub_resource type=\"ConcavePolygonShape3D\""),
            "the shape must be a real sub-resource:\n{text}"
        );
        assert!(
            text.contains("data = PackedVector3Array(0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0)"),
            "the shape must carry the real triangle soup:\n{text}"
        );
        assert!(text.contains("[node name=\"Ground\" type=\"StaticBody3D\" parent=\".\"]"));
        assert!(text.contains(
            "[node name=\"CollisionShape3D\" type=\"CollisionShape3D\" parent=\"Ground\"]"
        ));
    }

    #[test]
    fn tscn_writer_round_trips_through_packed_scene() {
        let mut anim = ImportedAnimation::new("Walk", 0.5, AnimationLoopMode::Linear);
        anim.tracks.push(ImportedAnimationTrack {
            node_path: "Body".into(),
            track_type: "position_3d".into(),
            interpolation: 1,
            keys: vec![0.0, 1.0, 0.0, 0.0, 0.0, 0.5, 1.0, 2.0, 0.0, 0.0],
        });
        let scene = ImportedScene {
            root: ImportedSceneNode::new("Char", "Node3D")
                .with_child(ImportedSceneNode::new("Body", "MeshInstance3D"))
                .with_child(ImportedSceneNode::new("AnimationPlayer", "AnimationPlayer")),
            animations: vec![anim],
            mesh_count: 1,
            warnings: Vec::new(),
        };

        let text = imported_scene_to_tscn(&scene);
        let packed = gdscene::packed_scene::PackedScene::from_tscn(&text)
            .unwrap_or_else(|e| panic!("the written scene must parse: {e}\n{text}"));
        assert_eq!(packed.node_count(), 3);

        let props: HashMap<String, Variant> = packed
            .instance()
            .unwrap()
            .iter()
            .find(|n| n.class_name() == "AnimationPlayer")
            .expect("the AnimationPlayer node must survive the round trip")
            .properties()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let player = gdscene::animation_tscn::build_animation_player(&packed, &props)
            .expect("the player must resolve its generated library");
        assert!(player.animations.contains_key("Walk"));
        assert_eq!(player.animations["Walk"].tracks.len(), 1);
    }

    #[test]
    fn import_scene_to_packed_scene_writes_under_godot_imported() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("model.gltf");
        std::fs::write(&source, NODES_ONLY_GLTF).unwrap();

        let reg = make_scene_registry();
        let out = import_scene_to_packed_scene(&reg, &source, dir.path()).unwrap();
        assert_eq!(out.parent().unwrap(), dir.path().join(".godot/imported"));
        assert_eq!(out.extension().and_then(|e| e.to_str()), Some("scn"));
        assert!(out
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap()
            .starts_with("model-"));

        let text = std::fs::read_to_string(&out).unwrap();
        assert!(gdscene::packed_scene::PackedScene::from_tscn(&text).is_ok());
    }
}
