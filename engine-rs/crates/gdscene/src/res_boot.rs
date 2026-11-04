//! Booting a project out of `res://` alone, with no filesystem (pat-mgixk.73).
//!
//! Godot's web export mounts the `.pck` and then runs the ordinary
//! `Main::setup` / `Main::start`, so the project's `application/run/main_scene`,
//! autoloads and `[input]` map are all live on the first frame
//! (`upstream/godot/platform/web/web_main.cpp`). Patina's web runtime built a
//! bare `Node2D` tree instead and left the mounted pack unread, so an exported
//! game booted an empty scene: no `_ready`, no `_process`, no input map.
//!
//! # Why this is not `patina-runner`'s `project_boot`
//!
//! That one is reachable only from the native binary, and `patina-web` cannot
//! depend on it. More importantly its entry points are *path*-based —
//! `boot_project(project_dir)` reads `project.godot` with `std::fs`, resolves a
//! fallback filesystem root for unpacked `res://`, loads the translation and
//! UID registries off disk, and installs path roots. None of that has a browser
//! equivalent.
//!
//! What this module does is the **path-independent** half, which turned out to
//! be small because the pieces it needs are already pack-aware:
//! [`FileAccess::read_file_string`](gdcore::file_access::FileAccess::read_file_string)
//! serves a mounted pack before disk, `remap_path` follows `.remap`/`.import`
//! through the same reader, and `PackedScene::from_tscn` +
//! `add_packed_scene_to_tree` never touch the filesystem at all.
//!
//! ⚠️ So the ONLY precondition is that a pack is already mounted (or a `res://`
//! root configured). This module reads `res://project.godot` and nothing else by
//! absolute path.
//!
//! # What it deliberately does not do
//!
//! Translations, the UID registry and `set_path_roots` are filesystem concerns
//! and are left to the native boot. A consequence worth knowing: a `uid://`
//! reference in `project.godot` is NOT resolved here, because the registry that
//! maps it lives on disk. An exported project referencing its main scene by
//! `uid://` will fail with a readable error rather than silently booting empty.

use std::path::Path;

use gdcore::file_access::FileAccess;
use gdcore::ProjectSettings;
use gdplatform::input::InputMap;
use gdresource::{AutoloadEntry, ProjectLoader};
use gdvariant::Variant;

use crate::node::{Node, NodeId};
use crate::packed_scene::{add_packed_scene_to_tree, PackedScene};
use crate::scene_tree::SceneTree;

/// A project booted entirely out of `res://`.
pub struct BootedRes {
    /// The tree with autoloads and the main scene instanced under `/root`.
    pub tree: SceneTree,
    /// Settings parsed from `project.godot`.
    pub settings: ProjectSettings,
    /// The `[input]` actions the project declares, ready for
    /// `MainLoop::set_input_map`.
    ///
    /// Built with `InputMap::load_from_project_godot`, the same call the native
    /// boot uses, so the two paths cannot disagree about what an action means.
    pub input_map: InputMap,
    /// Root of the instanced main scene, if `run/main_scene` was set.
    pub main_scene_root: Option<NodeId>,
    /// `(autoload_name, node_id)` in declared order.
    pub autoload_roots: Vec<(String, NodeId)>,
}

/// Instances `res_path` under `parent`, reading it through `FileAccess` so a
/// mounted `.pck` is served before disk.
///
/// ⚠️ `remap_path` first: a default Godot export replaces every text resource
/// with a `<path>.remap` pointing at the converted binary, so the authored
/// `res://main.tscn` is not what is actually in the pack.
pub fn load_scene_into_tree(
    tree: &mut SceneTree,
    parent: NodeId,
    res_path: &str,
) -> Result<NodeId, String> {
    let res_path = &gdresource::importers::remap_path(res_path);
    let source = FileAccess::read_file_string(res_path)
        .map_err(|e| format!("failed to read scene '{res_path}': {e}"))?;
    let packed = PackedScene::from_tscn(&source)
        .map_err(|e| format!("failed to parse scene '{res_path}': {e}"))?;
    // A RUNTIME load: every `instance=` sub-scene is expanded. The plain
    // instancer leaves them as childless placeholders (the editor's shape).
    crate::packed_scene::add_packed_scene_to_tree_from_res(tree, parent, &packed)
        .map_err(|e| format!("failed to instance scene '{res_path}': {e}"))
}

/// Instances one autoload, mirroring Godot's rule that a `.tscn`/`.scn`
/// autoload is a scene while anything else is a bare `Node` carrying the script.
///
/// The node is renamed to the autoload's registered name afterwards, because
/// that is the name scripts reach it by (`/root/<Name>`), not the scene's own
/// root name.
pub fn instantiate_autoload(
    tree: &mut SceneTree,
    parent: NodeId,
    entry: &AutoloadEntry,
) -> Result<NodeId, String> {
    let path = &entry.path;
    let node_id = if path.ends_with(".tscn") || path.ends_with(".scn") {
        load_scene_into_tree(tree, parent, path)?
    } else {
        let mut node = Node::new(entry.name.clone(), "Node");
        node.set_property("script", Variant::String(path.clone()));
        tree.add_child(parent, node)
            .map_err(|e| format!("failed to add autoload '{}': {e}", entry.name))?
    };
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_name(entry.name.clone());
    }
    Ok(node_id)
}

/// Boots the project described by `res://project.godot`.
///
/// Requires a mounted pack (or configured `res://` root) and nothing else. The
/// returned tree has autoloads and the main scene instanced, with every
/// `_script_path` attached, but **no lifecycle has run**: the caller owns
/// `enter_tree`, exactly as the native runner does, so it can install its
/// `MainLoop` configuration first.
///
/// ⚠️ Autoloads are instanced BEFORE the main scene and returned in declared
/// order, because Godot guarantees an autoload is in the tree before the main
/// scene's `_ready` can reach it. A caller that enters the main scene first
/// would break `get_node("/root/MyAutoload")` in `_ready`.
pub fn boot_from_res() -> Result<BootedRes, String> {
    // (0) ClassDB before anything loads a scene. The loader seeds each node's
    // defaults from `class_db::get_property_list()` as the node is built, so
    // registering afterwards leaves every already-loaded node bare — a failure
    // that looks like a ClassDB bug rather than an ordering bug. Idempotent.
    gdobject::class_db::register_all_classes();

    let content = FileAccess::read_file_string("res://project.godot")
        .map_err(|e| format!("failed to read res://project.godot (is a pack mounted?): {e}"))?;

    // `project_root` is stored by the loader but not consulted while parsing;
    // there is no filesystem root in a browser, so the scheme itself stands in.
    let loader = ProjectLoader::from_text(Path::new("res://"), &content)
        .map_err(|e| format!("failed to parse project.godot: {e}"))?;
    let settings = ProjectSettings::from_godot_format(&content)
        .map_err(|e| format!("failed to load ProjectSettings: {e}"))?;

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    let mut autoload_roots = Vec::new();
    for entry in loader.get_autoloads() {
        let node_id = instantiate_autoload(&mut tree, root_id, &entry)?;
        autoload_roots.push((entry.name, node_id));
    }

    let main_scene = loader.config().main_scene.clone();
    let main_scene_root = if main_scene.is_empty() {
        None
    } else {
        Some(load_scene_into_tree(&mut tree, root_id, &main_scene)?)
    };

    // Every node's `_script_path` becomes a live script. Without this the tree
    // is inert — the exact shape of pat-mgixk.61, where a spawned scene's
    // scripts never ran. Done for the autoloads too, since a script autoload
    // is the common case.
    for (_, id) in &autoload_roots {
        crate::scripting::attach_subtree_scripts_from_res(&mut tree, *id);
    }
    if let Some(root) = main_scene_root {
        crate::scripting::attach_subtree_scripts_from_res(&mut tree, root);
    }

    Ok(BootedRes {
        tree,
        settings,
        input_map: InputMap::load_from_project_godot(&content),
        main_scene_root,
        autoload_roots,
    })
}
