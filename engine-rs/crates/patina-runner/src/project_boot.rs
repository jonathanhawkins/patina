//! Boot a patina-runner project from a `project.godot` file (pat-nzgdu.17).
//!
//! Godot boots a project by parsing `project.godot`: it resolves
//! `application/run/main_scene`, registers `[autoload]` scene/script singletons
//! under `/root`, installs the `[input]` action map, and populates
//! [`ProjectSettings`] (physics/rendering/audio) that the runtime honors.
//!
//! Patina's runner previously took a raw `.tscn` path and ignored
//! `project.godot` entirely; the parsers existed but had no runtime caller and
//! `gdcore::set_path_roots` was never invoked, so `res://` / `user://` could not
//! resolve. This module implements that boot path:
//!
//! 1. parse `project.godot` ([`ProjectLoader`] for scene/autoload/input,
//!    [`ProjectSettings`] for config),
//! 2. call [`gdcore::set_path_roots`] so `FileAccess` `res://` works,
//! 3. instantiate `[autoload]` singletons under `/root` (before the main scene),
//! 4. resolve and load `run/main_scene`,
//! 5. build the `[input]` action map.

use std::path::{Path, PathBuf};

use gdcore::{resolve_path, set_path_roots, ProjectSettings};
use gdplatform::InputMap;
use gdresource::{AutoloadEntry, ProjectLoader};
use gdscene::{add_packed_scene_to_tree, Node, NodeId, PackedScene, SceneTree};
use gdvariant::Variant;

/// A project booted from `project.godot`: the populated scene tree plus the
/// [`ProjectSettings`] and [`InputMap`] the runtime should honor.
pub struct BootedProject {
    /// The scene tree with autoloads (under `/root`) and the main scene instanced.
    pub tree: SceneTree,
    /// Settings parsed from `project.godot` (physics/rendering/audio/...).
    pub settings: ProjectSettings,
    /// The `InputMap` actions declared in the `[input]` section.
    pub input_map: InputMap,
    /// Root node of the instanced main scene, if `run/main_scene` was set.
    pub main_scene_root: Option<NodeId>,
    /// `(autoload_name, node_id)` for each instanced autoload, in declared order.
    pub autoload_roots: Vec<(String, NodeId)>,
}

/// Boots the project rooted at `project_dir` (which must contain `project.godot`).
pub fn boot_project(project_dir: &Path) -> Result<BootedProject, String> {
    boot_project_with_user_root(project_dir, None)
}

/// Like [`boot_project`], but with an explicit `user://` root (e.g. the platform
/// application-data directory). Passing `None` leaves `user://` unmapped.
pub fn boot_project_with_user_root(
    project_dir: &Path,
    user_dir: Option<PathBuf>,
) -> Result<BootedProject, String> {
    let project_root = project_dir.to_path_buf();

    // (1) Parse project.godot for scene/autoloads/input and for ProjectSettings.
    let loader = ProjectLoader::load(&project_root).map_err(|e| {
        format!(
            "failed to load project.godot from {}: {e}",
            project_root.display()
        )
    })?;
    let settings = ProjectSettings::load_godot_project(&project_root.join("project.godot"))
        .map_err(|e| format!("failed to load ProjectSettings: {e}"))?;

    // (2) Wire res:// / user:// roots so FileAccess (and res:// scene loads) resolve.
    set_path_roots(Some(project_root.clone()), user_dir);

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    // (3) Instantiate autoloads BEFORE the main scene, each under /root and
    // renamed to its autoload name (Godot exposes them as `/root/<Name>`).
    let mut autoload_roots = Vec::new();
    for entry in loader.get_autoloads() {
        let node_id = instantiate_autoload(&mut tree, root_id, &entry)?;
        autoload_roots.push((entry.name, node_id));
    }

    // (4) Resolve and instance the declared main scene.
    let main_scene = loader.config().main_scene.clone();
    let main_scene_root = if main_scene.is_empty() {
        None
    } else {
        Some(load_scene_into_tree(&mut tree, root_id, &main_scene)?)
    };

    // (5) Install the input map actions declared in `[input]`.
    let mut input_map = InputMap::new();
    for entry in loader.get_input_map() {
        // ProjectLoader keeps action names + raw event strings; project.godot
        // uses a per-action `dead_zone` (default 0.5 when unspecified).
        input_map.add_action(entry.action_name, 0.5_f32);
    }

    Ok(BootedProject {
        tree,
        settings,
        input_map,
        main_scene_root,
        autoload_roots,
    })
}

/// Instantiates one autoload under `parent`, returning its node id. Scene
/// autoloads (`.tscn`/`.scn`) are instanced; other (script) autoloads become a
/// bare `Node` carrying the script path. Either way the node is renamed to the
/// autoload's name so it is reachable as `/root/<Name>`.
fn instantiate_autoload(
    tree: &mut SceneTree,
    parent: NodeId,
    entry: &AutoloadEntry,
) -> Result<NodeId, String> {
    let node_id = if entry.path.ends_with(".tscn") || entry.path.ends_with(".scn") {
        load_scene_into_tree(tree, parent, &entry.path)?
    } else {
        let mut node = Node::new(entry.name.clone(), "Node");
        node.set_property("script", Variant::String(entry.path.clone()));
        tree.add_child(parent, node)
            .map_err(|e| format!("failed to add autoload '{}': {e}", entry.name))?
    };
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_name(entry.name.clone());
    }
    Ok(node_id)
}

/// Resolves a `res://` scene path to disk, parses the `.tscn`, and instances it
/// under `parent`, returning the instanced root node id.
fn load_scene_into_tree(
    tree: &mut SceneTree,
    parent: NodeId,
    res_path: &str,
) -> Result<NodeId, String> {
    let fs_path = resolve_path(res_path).map_err(|e| format!("failed to resolve '{res_path}': {e}"))?;
    let source = std::fs::read_to_string(&fs_path)
        .map_err(|e| format!("failed to read scene '{}': {e}", fs_path.display()))?;
    let packed = PackedScene::from_tscn(&source)
        .map_err(|e| format!("failed to parse scene '{}': {e}", fs_path.display()))?;
    add_packed_scene_to_tree(tree, parent, &packed)
        .map_err(|e| format!("failed to instance scene '{res_path}': {e}"))
}
