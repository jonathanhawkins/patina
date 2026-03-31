//! Asset browser drag-and-drop to scene viewport and inspector.
//!
//! Provides a drag-and-drop system connecting the FileSystemDock (asset browser)
//! to the 2D/3D viewports and the inspector panel. Supports:
//!
//! - Dragging scene files (`.tscn`, `.scn`) to instantiate as child nodes.
//! - Dragging textures to assign to sprite or material properties.
//! - Dragging scripts to attach to the selected node.
//! - Dragging audio/mesh/resource files to assign to inspector properties.
//! - Drop preview with validation feedback.

use crate::filesystem::{FileIcon, FileSystemEntry};

// ---------------------------------------------------------------------------
// Drag payload
// ---------------------------------------------------------------------------

/// The type of resource being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragResourceType {
    /// A scene file (.tscn, .scn) — can be instantiated.
    Scene,
    /// A texture/image file (.png, .jpg, .webp, .svg).
    Texture,
    /// A script file (.gd, .gdscript).
    Script,
    /// An audio file (.wav, .ogg, .mp3).
    Audio,
    /// A 3D mesh/model file (.glb, .gltf, .obj, .fbx).
    Mesh3D,
    /// A shader file (.gdshader, .shader).
    Shader,
    /// A generic resource file (.tres).
    Resource,
    /// An unknown/unsupported file type.
    Unknown,
}

impl DragResourceType {
    /// Infers the resource type from a file extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "tscn" | "scn" => DragResourceType::Scene,
            "png" | "jpg" | "jpeg" | "webp" | "svg" | "bmp" | "tga" => DragResourceType::Texture,
            "gd" | "gdscript" => DragResourceType::Script,
            "wav" | "ogg" | "mp3" => DragResourceType::Audio,
            "glb" | "gltf" | "obj" | "fbx" => DragResourceType::Mesh3D,
            "gdshader" | "shader" => DragResourceType::Shader,
            "tres" => DragResourceType::Resource,
            _ => DragResourceType::Unknown,
        }
    }

    /// Infers the resource type from a full file path.
    pub fn from_path(path: &str) -> Self {
        path.rsplit('.')
            .next()
            .map(Self::from_extension)
            .unwrap_or(DragResourceType::Unknown)
    }
}

/// Data carried during a drag operation.
#[derive(Debug, Clone, PartialEq)]
pub struct DragPayload {
    /// The res:// path of the dragged asset.
    pub res_path: String,
    /// The display name of the asset.
    pub display_name: String,
    /// The inferred resource type.
    pub resource_type: DragResourceType,
    /// The file icon for visual feedback.
    pub icon: FileIcon,
}

impl DragPayload {
    /// Creates a drag payload from a filesystem entry.
    pub fn from_entry(entry: &FileSystemEntry) -> Self {
        Self {
            res_path: entry.res_path.clone(),
            display_name: entry.name.clone(),
            resource_type: DragResourceType::from_path(&entry.res_path),
            icon: entry.icon,
        }
    }

    /// Creates a drag payload from a res:// path.
    pub fn from_res_path(res_path: impl Into<String>) -> Self {
        let res_path = res_path.into();
        let display_name = res_path.rsplit('/').next().unwrap_or(&res_path).to_string();
        let resource_type = DragResourceType::from_path(&res_path);
        let icon = res_path
            .rsplit('.')
            .next()
            .map(FileIcon::from_extension)
            .unwrap_or(FileIcon::Unknown);
        Self {
            res_path,
            display_name,
            resource_type,
            icon,
        }
    }
}

// ---------------------------------------------------------------------------
// Drop target
// ---------------------------------------------------------------------------

/// Where a drop can land.
#[derive(Debug, Clone, PartialEq)]
pub enum DropTarget {
    /// The 2D scene viewport at a screen position.
    Viewport2D { x: f32, y: f32 },
    /// The 3D scene viewport at a screen position.
    Viewport3D { x: f32, y: f32 },
    /// An inspector property field.
    InspectorProperty {
        /// The node being inspected.
        node_id: u64,
        /// The property name to assign.
        property_name: String,
    },
    /// The scene tree dock (reparent/instantiate).
    SceneTree {
        /// The parent node ID to instantiate under.
        parent_node_id: u64,
        /// Index among siblings (-1 = append).
        sibling_index: i32,
    },
}

// ---------------------------------------------------------------------------
// Drop validation
// ---------------------------------------------------------------------------

/// Whether a drop is valid and what it would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropValidity {
    /// The drop is valid and would produce the described action.
    Valid(DropAction),
    /// The drop is not valid (with a reason).
    Invalid(String),
}

/// What action a valid drop would perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropAction {
    /// Instantiate a scene as a child node.
    InstantiateScene,
    /// Create a new node with the resource (e.g., Sprite2D from texture).
    CreateNodeFromResource,
    /// Assign a resource to a property on the selected node.
    AssignProperty,
    /// Attach a script to the selected node.
    AttachScript,
}

/// Rules for which resource types can be dropped on which targets.
pub fn validate_drop(payload: &DragPayload, target: &DropTarget) -> DropValidity {
    match target {
        DropTarget::Viewport2D { .. } | DropTarget::Viewport3D { .. } => {
            match payload.resource_type {
                DragResourceType::Scene => DropValidity::Valid(DropAction::InstantiateScene),
                DragResourceType::Texture => {
                    DropValidity::Valid(DropAction::CreateNodeFromResource)
                }
                DragResourceType::Mesh3D => {
                    if matches!(target, DropTarget::Viewport3D { .. }) {
                        DropValidity::Valid(DropAction::CreateNodeFromResource)
                    } else {
                        DropValidity::Invalid("3D meshes cannot be dropped on a 2D viewport".into())
                    }
                }
                DragResourceType::Audio => DropValidity::Valid(DropAction::CreateNodeFromResource),
                DragResourceType::Script => DropValidity::Invalid(
                    "Scripts must be dropped on a node, not the viewport".into(),
                ),
                DragResourceType::Shader => {
                    DropValidity::Invalid("Shaders must be assigned to a material property".into())
                }
                DragResourceType::Resource => {
                    DropValidity::Invalid("Generic resources must be assigned to a property".into())
                }
                DragResourceType::Unknown => {
                    DropValidity::Invalid("Unknown file type cannot be dropped here".into())
                }
            }
        }

        DropTarget::InspectorProperty { property_name, .. } => {
            // Validate based on property name heuristics
            let prop = property_name.to_lowercase();
            match payload.resource_type {
                DragResourceType::Texture => {
                    if prop.contains("texture")
                        || prop.contains("sprite")
                        || prop.contains("icon")
                        || prop.contains("albedo")
                        || prop.contains("normal_map")
                        || prop.contains("emission")
                        || prop.contains("image")
                    {
                        DropValidity::Valid(DropAction::AssignProperty)
                    } else {
                        DropValidity::Invalid(format!(
                            "Texture cannot be assigned to property '{}'",
                            property_name
                        ))
                    }
                }
                DragResourceType::Script => {
                    if prop == "script" {
                        DropValidity::Valid(DropAction::AttachScript)
                    } else {
                        DropValidity::Invalid(
                            "Scripts can only be assigned to the 'script' property".into(),
                        )
                    }
                }
                DragResourceType::Audio => {
                    if prop.contains("stream") || prop.contains("audio") || prop.contains("sound") {
                        DropValidity::Valid(DropAction::AssignProperty)
                    } else {
                        DropValidity::Invalid(format!(
                            "Audio cannot be assigned to property '{}'",
                            property_name
                        ))
                    }
                }
                DragResourceType::Mesh3D => {
                    if prop.contains("mesh") {
                        DropValidity::Valid(DropAction::AssignProperty)
                    } else {
                        DropValidity::Invalid(format!(
                            "Mesh cannot be assigned to property '{}'",
                            property_name
                        ))
                    }
                }
                DragResourceType::Shader => {
                    if prop.contains("shader") || prop.contains("material") {
                        DropValidity::Valid(DropAction::AssignProperty)
                    } else {
                        DropValidity::Invalid(format!(
                            "Shader cannot be assigned to property '{}'",
                            property_name
                        ))
                    }
                }
                DragResourceType::Scene => {
                    DropValidity::Invalid("Scenes cannot be assigned as properties".into())
                }
                DragResourceType::Resource => {
                    // Generic resources can go to any resource-typed property
                    DropValidity::Valid(DropAction::AssignProperty)
                }
                DragResourceType::Unknown => {
                    DropValidity::Invalid("Unknown file type cannot be assigned".into())
                }
            }
        }

        DropTarget::SceneTree { .. } => match payload.resource_type {
            DragResourceType::Scene => DropValidity::Valid(DropAction::InstantiateScene),
            DragResourceType::Script => DropValidity::Valid(DropAction::AttachScript),
            _ => DropValidity::Invalid(
                "Only scenes and scripts can be dropped on the scene tree".into(),
            ),
        },
    }
}

/// Returns the default node type that would be created when dropping a resource
/// of the given type onto a viewport.
pub fn default_node_type_for_resource(
    resource_type: DragResourceType,
    is_3d: bool,
) -> Option<&'static str> {
    match resource_type {
        DragResourceType::Scene => Some("PackedScene"),
        DragResourceType::Texture => {
            if is_3d {
                Some("Sprite3D")
            } else {
                Some("Sprite2D")
            }
        }
        DragResourceType::Audio => {
            if is_3d {
                Some("AudioStreamPlayer3D")
            } else {
                Some("AudioStreamPlayer2D")
            }
        }
        DragResourceType::Mesh3D => {
            if is_3d {
                Some("MeshInstance3D")
            } else {
                None
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Drag state machine
// ---------------------------------------------------------------------------

/// The current phase of a drag-drop operation.
#[derive(Debug, Clone, PartialEq)]
pub enum DragPhase {
    /// No drag in progress.
    Idle,
    /// Dragging over a valid target.
    Dragging {
        payload: DragPayload,
        current_target: Option<DropTarget>,
        validity: Option<DropValidity>,
    },
}

/// Manages the drag-and-drop state for asset browser interactions.
#[derive(Debug)]
pub struct AssetDragDrop {
    /// Current drag phase.
    phase: DragPhase,
    /// History of completed drops for undo support.
    drop_history: Vec<CompletedDrop>,
    /// Maximum history length.
    max_history: usize,
}

/// Record of a completed drop for undo.
#[derive(Debug, Clone)]
pub struct CompletedDrop {
    /// The resource that was dropped.
    pub res_path: String,
    /// Where it was dropped.
    pub target: DropTarget,
    /// What action was performed.
    pub action: DropAction,
}

impl Default for AssetDragDrop {
    fn default() -> Self {
        Self {
            phase: DragPhase::Idle,
            drop_history: Vec::new(),
            max_history: 50,
        }
    }
}

impl AssetDragDrop {
    /// Creates a new drag-drop manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the current drag phase.
    pub fn phase(&self) -> &DragPhase {
        &self.phase
    }

    /// Returns true if a drag is currently in progress.
    pub fn is_dragging(&self) -> bool {
        !matches!(self.phase, DragPhase::Idle)
    }

    /// Returns the current drag payload, if any.
    pub fn payload(&self) -> Option<&DragPayload> {
        match &self.phase {
            DragPhase::Dragging { payload, .. } => Some(payload),
            DragPhase::Idle => None,
        }
    }

    /// Returns the current drop validity, if hovering over a target.
    pub fn current_validity(&self) -> Option<&DropValidity> {
        match &self.phase {
            DragPhase::Dragging { validity, .. } => validity.as_ref(),
            DragPhase::Idle => None,
        }
    }

    /// Returns true if the current hover position would accept a drop.
    pub fn can_drop(&self) -> bool {
        matches!(self.current_validity(), Some(DropValidity::Valid(_)))
    }

    /// Begins a drag operation from the filesystem dock.
    pub fn begin_drag(&mut self, entry: &FileSystemEntry) {
        if entry.is_directory {
            return; // Can't drag directories
        }
        self.phase = DragPhase::Dragging {
            payload: DragPayload::from_entry(entry),
            current_target: None,
            validity: None,
        };
    }

    /// Begins a drag from a res:// path directly.
    pub fn begin_drag_from_path(&mut self, res_path: impl Into<String>) {
        self.phase = DragPhase::Dragging {
            payload: DragPayload::from_res_path(res_path),
            current_target: None,
            validity: None,
        };
    }

    /// Updates the hover target during a drag. Recomputes validity.
    pub fn update_hover(&mut self, target: DropTarget) {
        if let DragPhase::Dragging {
            payload,
            current_target,
            validity,
        } = &mut self.phase
        {
            let new_validity = validate_drop(payload, &target);
            *current_target = Some(target);
            *validity = Some(new_validity);
        }
    }

    /// Clears the hover target (e.g., mouse left all drop zones).
    pub fn clear_hover(&mut self) {
        if let DragPhase::Dragging {
            current_target,
            validity,
            ..
        } = &mut self.phase
        {
            *current_target = None;
            *validity = None;
        }
    }

    /// Attempts to complete the drop at the current hover target.
    ///
    /// Returns `Some(CompletedDrop)` if the drop was valid and performed,
    /// `None` if the drop was invalid or no target was hovered.
    pub fn drop(&mut self) -> Option<CompletedDrop> {
        let phase = std::mem::replace(&mut self.phase, DragPhase::Idle);

        match phase {
            DragPhase::Dragging {
                payload,
                current_target: Some(target),
                validity: Some(DropValidity::Valid(action)),
            } => {
                let completed = CompletedDrop {
                    res_path: payload.res_path,
                    target,
                    action,
                };
                self.drop_history.push(completed.clone());
                if self.drop_history.len() > self.max_history {
                    self.drop_history.remove(0);
                }
                Some(completed)
            }
            _ => None,
        }
    }

    /// Cancels the current drag without dropping.
    pub fn cancel(&mut self) {
        self.phase = DragPhase::Idle;
    }

    /// Returns the drop history.
    pub fn drop_history(&self) -> &[CompletedDrop] {
        &self.drop_history
    }

    /// Returns the most recent drop, if any.
    pub fn last_drop(&self) -> Option<&CompletedDrop> {
        self.drop_history.last()
    }

    /// Clears drop history.
    pub fn clear_history(&mut self) {
        self.drop_history.clear();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(name: &str, res_path: &str, is_dir: bool) -> FileSystemEntry {
        FileSystemEntry {
            name: name.to_string(),
            relative_path: res_path.trim_start_matches("res://").to_string(),
            res_path: res_path.to_string(),
            is_directory: is_dir,
            icon: FileIcon::from_extension(res_path.rsplit('.').next().unwrap_or("")),
            depth: 0,
            expanded: false,
            child_count: 0,
        }
    }

    // -- DragResourceType --

    #[test]
    fn resource_type_from_extension() {
        assert_eq!(
            DragResourceType::from_extension("tscn"),
            DragResourceType::Scene
        );
        assert_eq!(
            DragResourceType::from_extension("scn"),
            DragResourceType::Scene
        );
        assert_eq!(
            DragResourceType::from_extension("png"),
            DragResourceType::Texture
        );
        assert_eq!(
            DragResourceType::from_extension("jpg"),
            DragResourceType::Texture
        );
        assert_eq!(
            DragResourceType::from_extension("gd"),
            DragResourceType::Script
        );
        assert_eq!(
            DragResourceType::from_extension("wav"),
            DragResourceType::Audio
        );
        assert_eq!(
            DragResourceType::from_extension("glb"),
            DragResourceType::Mesh3D
        );
        assert_eq!(
            DragResourceType::from_extension("gdshader"),
            DragResourceType::Shader
        );
        assert_eq!(
            DragResourceType::from_extension("tres"),
            DragResourceType::Resource
        );
        assert_eq!(
            DragResourceType::from_extension("xyz"),
            DragResourceType::Unknown
        );
    }

    #[test]
    fn resource_type_from_path() {
        assert_eq!(
            DragResourceType::from_path("res://scenes/main.tscn"),
            DragResourceType::Scene
        );
        assert_eq!(
            DragResourceType::from_path("res://icon.png"),
            DragResourceType::Texture
        );
        assert_eq!(
            DragResourceType::from_path("no_extension"),
            DragResourceType::Unknown
        );
    }

    #[test]
    fn resource_type_case_insensitive() {
        assert_eq!(
            DragResourceType::from_extension("PNG"),
            DragResourceType::Texture
        );
        assert_eq!(
            DragResourceType::from_extension("GD"),
            DragResourceType::Script
        );
        assert_eq!(
            DragResourceType::from_extension("TSCN"),
            DragResourceType::Scene
        );
    }

    // -- DragPayload --

    #[test]
    fn payload_from_entry() {
        let entry = make_entry("main.tscn", "res://scenes/main.tscn", false);
        let payload = DragPayload::from_entry(&entry);
        assert_eq!(payload.res_path, "res://scenes/main.tscn");
        assert_eq!(payload.display_name, "main.tscn");
        assert_eq!(payload.resource_type, DragResourceType::Scene);
    }

    #[test]
    fn payload_from_res_path() {
        let payload = DragPayload::from_res_path("res://textures/sprite.png");
        assert_eq!(payload.display_name, "sprite.png");
        assert_eq!(payload.resource_type, DragResourceType::Texture);
    }

    // -- Drop validation: viewport --

    #[test]
    fn validate_scene_on_viewport_2d() {
        let payload = DragPayload::from_res_path("res://scenes/enemy.tscn");
        let target = DropTarget::Viewport2D { x: 100.0, y: 200.0 };
        assert_eq!(
            validate_drop(&payload, &target),
            DropValidity::Valid(DropAction::InstantiateScene),
        );
    }

    #[test]
    fn validate_texture_on_viewport_2d() {
        let payload = DragPayload::from_res_path("res://sprites/player.png");
        let target = DropTarget::Viewport2D { x: 100.0, y: 200.0 };
        assert_eq!(
            validate_drop(&payload, &target),
            DropValidity::Valid(DropAction::CreateNodeFromResource),
        );
    }

    #[test]
    fn validate_mesh_on_viewport_3d() {
        let payload = DragPayload::from_res_path("res://models/tree.glb");
        let target = DropTarget::Viewport3D { x: 100.0, y: 200.0 };
        assert_eq!(
            validate_drop(&payload, &target),
            DropValidity::Valid(DropAction::CreateNodeFromResource),
        );
    }

    #[test]
    fn validate_mesh_on_viewport_2d_rejected() {
        let payload = DragPayload::from_res_path("res://models/tree.glb");
        let target = DropTarget::Viewport2D { x: 100.0, y: 200.0 };
        assert!(matches!(
            validate_drop(&payload, &target),
            DropValidity::Invalid(_),
        ));
    }

    #[test]
    fn validate_script_on_viewport_rejected() {
        let payload = DragPayload::from_res_path("res://scripts/player.gd");
        let target = DropTarget::Viewport2D { x: 0.0, y: 0.0 };
        assert!(matches!(
            validate_drop(&payload, &target),
            DropValidity::Invalid(_),
        ));
    }

    #[test]
    fn validate_audio_on_viewport() {
        let payload = DragPayload::from_res_path("res://audio/bgm.ogg");
        let target = DropTarget::Viewport3D { x: 0.0, y: 0.0 };
        assert_eq!(
            validate_drop(&payload, &target),
            DropValidity::Valid(DropAction::CreateNodeFromResource),
        );
    }

    // -- Drop validation: inspector --

    #[test]
    fn validate_texture_on_texture_property() {
        let payload = DragPayload::from_res_path("res://sprites/icon.png");
        let target = DropTarget::InspectorProperty {
            node_id: 1,
            property_name: "texture".into(),
        };
        assert_eq!(
            validate_drop(&payload, &target),
            DropValidity::Valid(DropAction::AssignProperty),
        );
    }

    #[test]
    fn validate_texture_on_wrong_property() {
        let payload = DragPayload::from_res_path("res://sprites/icon.png");
        let target = DropTarget::InspectorProperty {
            node_id: 1,
            property_name: "position".into(),
        };
        assert!(matches!(
            validate_drop(&payload, &target),
            DropValidity::Invalid(_),
        ));
    }

    #[test]
    fn validate_script_on_script_property() {
        let payload = DragPayload::from_res_path("res://scripts/player.gd");
        let target = DropTarget::InspectorProperty {
            node_id: 1,
            property_name: "script".into(),
        };
        assert_eq!(
            validate_drop(&payload, &target),
            DropValidity::Valid(DropAction::AttachScript),
        );
    }

    #[test]
    fn validate_audio_on_stream_property() {
        let payload = DragPayload::from_res_path("res://audio/sfx.wav");
        let target = DropTarget::InspectorProperty {
            node_id: 1,
            property_name: "stream".into(),
        };
        assert_eq!(
            validate_drop(&payload, &target),
            DropValidity::Valid(DropAction::AssignProperty),
        );
    }

    #[test]
    fn validate_mesh_on_mesh_property() {
        let payload = DragPayload::from_res_path("res://models/cube.obj");
        let target = DropTarget::InspectorProperty {
            node_id: 1,
            property_name: "mesh".into(),
        };
        assert_eq!(
            validate_drop(&payload, &target),
            DropValidity::Valid(DropAction::AssignProperty),
        );
    }

    #[test]
    fn validate_shader_on_material_property() {
        let payload = DragPayload::from_res_path("res://shaders/glow.gdshader");
        let target = DropTarget::InspectorProperty {
            node_id: 1,
            property_name: "material".into(),
        };
        assert_eq!(
            validate_drop(&payload, &target),
            DropValidity::Valid(DropAction::AssignProperty),
        );
    }

    // -- Drop validation: scene tree --

    #[test]
    fn validate_scene_on_scene_tree() {
        let payload = DragPayload::from_res_path("res://scenes/enemy.tscn");
        let target = DropTarget::SceneTree {
            parent_node_id: 1,
            sibling_index: -1,
        };
        assert_eq!(
            validate_drop(&payload, &target),
            DropValidity::Valid(DropAction::InstantiateScene),
        );
    }

    #[test]
    fn validate_script_on_scene_tree() {
        let payload = DragPayload::from_res_path("res://scripts/enemy.gd");
        let target = DropTarget::SceneTree {
            parent_node_id: 1,
            sibling_index: -1,
        };
        assert_eq!(
            validate_drop(&payload, &target),
            DropValidity::Valid(DropAction::AttachScript),
        );
    }

    #[test]
    fn validate_texture_on_scene_tree_rejected() {
        let payload = DragPayload::from_res_path("res://textures/bg.png");
        let target = DropTarget::SceneTree {
            parent_node_id: 1,
            sibling_index: -1,
        };
        assert!(matches!(
            validate_drop(&payload, &target),
            DropValidity::Invalid(_),
        ));
    }

    // -- default_node_type_for_resource --

    #[test]
    fn default_node_type_texture_2d() {
        assert_eq!(
            default_node_type_for_resource(DragResourceType::Texture, false),
            Some("Sprite2D"),
        );
    }

    #[test]
    fn default_node_type_texture_3d() {
        assert_eq!(
            default_node_type_for_resource(DragResourceType::Texture, true),
            Some("Sprite3D"),
        );
    }

    #[test]
    fn default_node_type_audio_2d() {
        assert_eq!(
            default_node_type_for_resource(DragResourceType::Audio, false),
            Some("AudioStreamPlayer2D"),
        );
    }

    #[test]
    fn default_node_type_mesh_3d() {
        assert_eq!(
            default_node_type_for_resource(DragResourceType::Mesh3D, true),
            Some("MeshInstance3D"),
        );
    }

    #[test]
    fn default_node_type_mesh_2d_none() {
        assert_eq!(
            default_node_type_for_resource(DragResourceType::Mesh3D, false),
            None,
        );
    }

    #[test]
    fn default_node_type_script_none() {
        assert_eq!(
            default_node_type_for_resource(DragResourceType::Script, false),
            None,
        );
    }

    // -- AssetDragDrop state machine --

    #[test]
    fn drag_drop_starts_idle() {
        let dd = AssetDragDrop::new();
        assert!(!dd.is_dragging());
        assert!(dd.payload().is_none());
        assert!(!dd.can_drop());
    }

    #[test]
    fn begin_drag_from_entry() {
        let mut dd = AssetDragDrop::new();
        let entry = make_entry("player.tscn", "res://scenes/player.tscn", false);
        dd.begin_drag(&entry);
        assert!(dd.is_dragging());
        let payload = dd.payload().unwrap();
        assert_eq!(payload.res_path, "res://scenes/player.tscn");
        assert_eq!(payload.resource_type, DragResourceType::Scene);
    }

    #[test]
    fn begin_drag_ignores_directories() {
        let mut dd = AssetDragDrop::new();
        let entry = make_entry("scenes", "res://scenes", true);
        dd.begin_drag(&entry);
        assert!(!dd.is_dragging());
    }

    #[test]
    fn begin_drag_from_path() {
        let mut dd = AssetDragDrop::new();
        dd.begin_drag_from_path("res://icon.png");
        assert!(dd.is_dragging());
        assert_eq!(
            dd.payload().unwrap().resource_type,
            DragResourceType::Texture
        );
    }

    #[test]
    fn update_hover_validates() {
        let mut dd = AssetDragDrop::new();
        dd.begin_drag_from_path("res://scenes/enemy.tscn");
        dd.update_hover(DropTarget::Viewport2D { x: 100.0, y: 200.0 });
        assert!(dd.can_drop());
    }

    #[test]
    fn update_hover_invalid() {
        let mut dd = AssetDragDrop::new();
        dd.begin_drag_from_path("res://scripts/player.gd");
        dd.update_hover(DropTarget::Viewport2D { x: 0.0, y: 0.0 });
        assert!(!dd.can_drop());
    }

    #[test]
    fn clear_hover_removes_validity() {
        let mut dd = AssetDragDrop::new();
        dd.begin_drag_from_path("res://scenes/enemy.tscn");
        dd.update_hover(DropTarget::Viewport2D { x: 100.0, y: 200.0 });
        assert!(dd.can_drop());
        dd.clear_hover();
        assert!(!dd.can_drop());
    }

    #[test]
    fn drop_succeeds_on_valid_target() {
        let mut dd = AssetDragDrop::new();
        dd.begin_drag_from_path("res://scenes/enemy.tscn");
        dd.update_hover(DropTarget::Viewport2D { x: 100.0, y: 200.0 });
        let result = dd.drop();
        assert!(result.is_some());
        let completed = result.unwrap();
        assert_eq!(completed.res_path, "res://scenes/enemy.tscn");
        assert_eq!(completed.action, DropAction::InstantiateScene);
        assert!(!dd.is_dragging());
    }

    #[test]
    fn drop_fails_on_invalid_target() {
        let mut dd = AssetDragDrop::new();
        dd.begin_drag_from_path("res://scripts/player.gd");
        dd.update_hover(DropTarget::Viewport2D { x: 0.0, y: 0.0 });
        let result = dd.drop();
        assert!(result.is_none());
        assert!(!dd.is_dragging());
    }

    #[test]
    fn drop_fails_with_no_target() {
        let mut dd = AssetDragDrop::new();
        dd.begin_drag_from_path("res://scenes/enemy.tscn");
        // No update_hover call
        let result = dd.drop();
        assert!(result.is_none());
    }

    #[test]
    fn cancel_drag() {
        let mut dd = AssetDragDrop::new();
        dd.begin_drag_from_path("res://scenes/enemy.tscn");
        assert!(dd.is_dragging());
        dd.cancel();
        assert!(!dd.is_dragging());
    }

    #[test]
    fn drop_history_tracks_completed_drops() {
        let mut dd = AssetDragDrop::new();
        assert!(dd.drop_history().is_empty());

        dd.begin_drag_from_path("res://scenes/a.tscn");
        dd.update_hover(DropTarget::Viewport2D { x: 0.0, y: 0.0 });
        dd.drop();

        dd.begin_drag_from_path("res://scenes/b.tscn");
        dd.update_hover(DropTarget::Viewport3D { x: 0.0, y: 0.0 });
        dd.drop();

        assert_eq!(dd.drop_history().len(), 2);
        assert_eq!(dd.last_drop().unwrap().res_path, "res://scenes/b.tscn");
    }

    #[test]
    fn drop_history_capped() {
        let mut dd = AssetDragDrop::new();
        dd.max_history = 3;

        for i in 0..5 {
            dd.begin_drag_from_path(format!("res://scenes/{}.tscn", i));
            dd.update_hover(DropTarget::Viewport2D { x: 0.0, y: 0.0 });
            dd.drop();
        }

        assert_eq!(dd.drop_history().len(), 3);
        assert_eq!(dd.drop_history()[0].res_path, "res://scenes/2.tscn");
    }

    #[test]
    fn clear_history() {
        let mut dd = AssetDragDrop::new();
        dd.begin_drag_from_path("res://scenes/a.tscn");
        dd.update_hover(DropTarget::Viewport2D { x: 0.0, y: 0.0 });
        dd.drop();
        assert!(!dd.drop_history().is_empty());
        dd.clear_history();
        assert!(dd.drop_history().is_empty());
    }

    #[test]
    fn cancelled_drag_not_in_history() {
        let mut dd = AssetDragDrop::new();
        dd.begin_drag_from_path("res://scenes/a.tscn");
        dd.update_hover(DropTarget::Viewport2D { x: 0.0, y: 0.0 });
        dd.cancel();
        assert!(dd.drop_history().is_empty());
    }
}
