//! Project autoload (global singleton) instantiation at engine boot.
//!
//! Godot instantiates each `[autoload]` entry declared in `project.godot` and
//! adds it under `/root` **before** the main scene, so the main scene's
//! `_ready` (and every node's `_ready`) can reference the autoloads by their
//! global path (e.g. `/root/Global`). See `main/main.cpp:4367-4433` in Godot:
//! for each autoload, in declaration order, Godot either instances the scene
//! (`PackedScene.instantiate()` for a `.tscn`/`.scn` path) or loads the script,
//! constructs a node of its base type and attaches the script, sets the node's
//! name to the autoload name, and adds it as a child of the tree root.
//!
//! This module replicates that for the Patina runtime. It is deliberately
//! placed in `gdscene` (rather than the runner binary) because **both** the
//! headless runner and the editor's "Play" path need it, and both depend on
//! `gdscene`.
//!
//! Two entry points:
//! - [`instantiate_autoloads`] — the low-level step: construct each autoload
//!   node, attach its script(s), and add it under a given root. It does **not**
//!   run lifecycle notifications, so a caller that owns its own `_ready`
//!   dispatch (the editor) can drive them afterwards.
//! - [`boot_autoloads_from_dir`] — the convenience used by the runner and the
//!   bootstrap phase: find `project.godot`, load its autoloads, instantiate
//!   them, and immediately run `enter_tree`/`_ready` on each so a subsequently
//!   added main scene observes them live.

use std::path::{Path, PathBuf};

use gdresource::{AutoloadEntry, ProjectLoader};
use gdvariant::Variant;

use crate::lifecycle::LifecycleManager;
use crate::node::{Node, NodeId};
use crate::packed_scene::{add_packed_scene_to_tree, PackedScene};
use crate::scene_tree::SceneTree;
use crate::scripting::GDScriptNodeInstance;

// ---------------------------------------------------------------------------
// AutoloadResult
// ---------------------------------------------------------------------------

/// Outcome of instantiating a project's autoloads.
#[derive(Debug, Default, Clone)]
pub struct AutoloadResult {
    /// `(autoload name, root node id)` for each successfully instantiated
    /// entry, in instantiation order.
    pub nodes: Vec<(String, NodeId)>,
    /// One line per autoload that failed to load or whose script failed to
    /// compile. These went to `tracing::warn!` only, so a typo in a global
    /// singleton's script left every `Global.x` reading null with nothing on
    /// screen; a host (the editor's Play) surfaces them.
    pub errors: Vec<String>,
}

impl AutoloadResult {
    /// Number of autoloads instantiated.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether no autoloads were instantiated.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Looks up an instantiated autoload's node id by its name.
    pub fn node_id(&self, name: &str) -> Option<NodeId> {
        self.nodes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Instantiates `autoloads` under `root_id` in the order given.
///
/// For each entry:
/// - a `.tscn`/`.scn` `path` is parsed and instanced under the root, and the
///   instanced scene's root node is renamed to the autoload name;
/// - any other `path` is treated as a GDScript (`.gd`): the source is read, a
///   node whose class is the script's `extends` base (defaulting to `Node`) is
///   created, named after the autoload, and the script is attached.
///
/// Scripts declared on the instanced nodes (both the `.gd` autoload itself and
/// any `_script_path` inside a `.tscn` autoload's subtree) are attached here.
///
/// This does **not** run `enter_tree`/`_ready`; the caller decides when to
/// dispatch lifecycle. `project_root` is used to resolve `res://` paths.
///
/// Missing files or parse failures are logged and skipped (non-fatal), matching
/// the runner's lenient boot behaviour.
pub fn instantiate_autoloads(
    tree: &mut SceneTree,
    root_id: NodeId,
    autoloads: &[AutoloadEntry],
    project_root: &Path,
) -> AutoloadResult {
    let mut result = AutoloadResult::default();
    for entry in autoloads {
        match instantiate_one(tree, root_id, entry, project_root, &mut result.errors) {
            Some(node_id) => {
                // Bind the autoload's global name so scripts can write
                // `Global.score` without a get_node (pat-nzgdu.70). Registered
                // per successful instantiation only: a name whose resource
                // failed to load has no node to resolve to, so leaving it
                // unregistered keeps `Global` an UndefinedVariable error rather
                // than a silent nil.
                gdscript_interop::autoload_registry::register_autoload(entry.name.clone());
                result.nodes.push((entry.name.clone(), node_id));
            }
            None => {
                tracing::warn!(
                    autoload = %entry.name,
                    path = %entry.path,
                    "autoload: failed to instantiate; skipping"
                );
                result.errors.push(format!(
                    "autoload {}: failed to load {}",
                    entry.name, entry.path
                ));
            }
        }
    }
    result
}

/// Finds `project.godot` at or above `start_dir`, loads its `[autoload]`
/// entries, instantiates them under the tree root, and runs
/// `enter_tree`/`_ready` on each.
///
/// Because every autoload node is constructed and added **before** any
/// `enter_tree` runs, an autoload's `_ready` can safely reference other
/// autoloads by their `/root/<Name>` path, and a main scene added afterwards
/// observes all of them in its own `_ready` — matching Godot's boot order.
///
/// Returns an empty [`AutoloadResult`] when no `project.godot` is found or it
/// declares no autoloads (e.g. headless scene-only runs).
pub fn boot_autoloads_from_dir(tree: &mut SceneTree, start_dir: &Path) -> AutoloadResult {
    let Some(project_root) = find_project_root(start_dir) else {
        return AutoloadResult::default();
    };
    let autoloads = match ProjectLoader::load(&project_root) {
        Ok(loader) => loader.get_autoloads(),
        Err(e) => {
            tracing::warn!(
                project = %project_root.display(),
                error = %e,
                "autoload: failed to load project.godot; no autoloads"
            );
            return AutoloadResult::default();
        }
    };
    if autoloads.is_empty() {
        return AutoloadResult::default();
    }

    let root_id = tree.root_id();
    let result = instantiate_autoloads(tree, root_id, &autoloads, &project_root);

    // Now that every autoload node exists in the tree, dispatch lifecycle in
    // instantiation order so cross-references resolve and any later main scene
    // sees them live.
    for (_, node_id) in &result.nodes {
        // `instantiate_one` adds the node **before** attaching its script (and,
        // for a `.tscn` autoload, before its children exist), so `add_child`
        // already ran a scriptless enter/ready pass over this subtree. Rewind it
        // or the node logs ENTER_TREE and READY twice and a scene autoload's
        // children ready parent-first (pat-0q2kx.46).
        LifecycleManager::rewind_for_redispatch(tree, *node_id);
        LifecycleManager::enter_tree(tree, *node_id);
    }
    result
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Instantiates a single autoload node (scene or script) under `root_id` and
/// returns its node id. Returns `None` if the resource can't be read/parsed.
fn instantiate_one(
    tree: &mut SceneTree,
    root_id: NodeId,
    entry: &AutoloadEntry,
    project_root: &Path,
    errors: &mut Vec<String>,
) -> Option<NodeId> {
    let fs_path = resolve_res_path(project_root, &entry.path);
    let lower = entry.path.to_ascii_lowercase();

    if lower.ends_with(".tscn") || lower.ends_with(".scn") {
        // Scene autoload: instance the packed scene and rename its root.
        let source = std::fs::read_to_string(&fs_path).ok()?;
        let scene = PackedScene::from_tscn(&source).ok()?;
        // Runtime load: expand the autoload scene's own `instance=` children.
        let node_id =
            crate::packed_scene::add_packed_scene_to_tree_from_res(tree, root_id, &scene).ok()?;
        if let Some(node) = tree.get_node_mut(node_id) {
            node.set_name(entry.name.clone());
        }
        // Attach any scripts declared inside the instanced subtree so the
        // scene autoload is fully live.
        attach_subtree_scripts(tree, node_id, project_root);
        Some(node_id)
    } else {
        // Script autoload (.gd): create a node of the script's base class and
        // attach the parsed script.
        let source = std::fs::read_to_string(&fs_path).ok()?;
        let base_class = base_class_from_source(&source);
        let node = Node::new(entry.name.clone(), base_class);
        let node_id = tree.add_child(root_id, node).ok()?;
        match GDScriptNodeInstance::from_source(&source, node_id) {
            Ok(mut instance) => {
                // So `get_script().resource_path` (and any tool listing the
                // tree) names the autoload's file, as for a scene script.
                gdscript_interop::bindings::ScriptInstance::set_script_file_path(
                    &mut instance,
                    &entry.path,
                );
                tree.attach_script(node_id, Box::new(instance))
            }
            Err(e) => {
                tracing::warn!(
                    autoload = %entry.name,
                    error = ?e,
                    "autoload: script parse failed; node added without script"
                );
                errors.push(format!(
                    "autoload {}: script {} failed to load: {}",
                    entry.name, entry.path, e
                ));
            }
        }
        Some(node_id)
    }
}

/// Walks the subtree rooted at `subtree_root` and attaches a
/// [`GDScriptNodeInstance`] to every node that carries a `_script_path`
/// property and does not already have a script. Used for `.tscn` autoloads.
fn attach_subtree_scripts(tree: &mut SceneTree, subtree_root: NodeId, project_root: &Path) {
    // Collect node ids first to avoid holding a borrow across mutation.
    let mut ids = Vec::new();
    let mut stack = vec![subtree_root];
    while let Some(id) = stack.pop() {
        if let Some(node) = tree.get_node(id) {
            for &child in node.children() {
                stack.push(child);
            }
            ids.push(id);
        }
    }

    let mut to_load: Vec<(NodeId, PathBuf)> = Vec::new();
    for id in ids {
        if tree.get_script(id).is_some() {
            continue;
        }
        if let Some(node) = tree.get_node(id) {
            if let Variant::String(res_path) = node.get_property("_script_path") {
                to_load.push((id, resolve_res_path(project_root, &res_path)));
            }
        }
    }

    for (id, path) in to_load {
        match std::fs::read_to_string(&path) {
            Ok(source) => {
                if let Ok(instance) = GDScriptNodeInstance::from_source(&source, id) {
                    tree.attach_script(id, Box::new(instance));
                }
            }
            Err(_) => { /* Non-fatal: missing script in autoload subtree. */ }
        }
    }
}

/// Extracts the base class from a GDScript's `extends` declaration.
///
/// Returns `Node` when there is no `extends`, or when the base is a path/`.gd`
/// reference (which resolves to a Node subtype we can't name statically — the
/// GDScript name-resolution companion bead refines that case). Only the first
/// `extends` line is honoured, matching GDScript grammar.
fn base_class_from_source(source: &str) -> String {
    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("extends") {
            // Require a separator so we don't match `extends_foo`.
            let rest = match rest.strip_prefix([' ', '\t']) {
                Some(r) => r.trim(),
                None => return "Node".to_string(),
            };
            let token = rest.split_whitespace().next().unwrap_or("Node");
            let token = token.trim_matches('"');
            if token.is_empty() || token.contains("://") || token.ends_with(".gd") {
                return "Node".to_string();
            }
            return token.to_string();
        }
        // The first meaningful line was not `extends`; GDScript allows
        // `class_name` before `extends`, so keep scanning a little, but a
        // `func`/`var`/other statement means there is no `extends`.
        if line.starts_with("class_name") || line.starts_with("@") {
            continue;
        }
        break;
    }
    "Node".to_string()
}

/// Resolves a `res://` path to a filesystem path, searching ancestor
/// directories so a project nested under the given root still resolves.
fn resolve_res_path(project_root: &Path, res_path: &str) -> PathBuf {
    let relative = res_path.strip_prefix("res://").unwrap_or(res_path);
    let candidate = project_root.join(relative);
    if candidate.exists() {
        return candidate;
    }
    for ancestor in project_root.ancestors().skip(1) {
        let candidate = ancestor.join(relative);
        if candidate.exists() {
            return candidate;
        }
    }
    project_root.join(relative)
}

/// Walks upward from `start_dir` looking for a directory containing
/// `project.godot`.
fn find_project_root(start_dir: &Path) -> Option<PathBuf> {
    for ancestor in start_dir.ancestors() {
        if ancestor.join("project.godot").is_file() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Creates a fresh, unique temp directory for a test fixture.
    fn unique_temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "patina_autoload_{}_{}_{}",
            tag,
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(name: &str, path: &str, singleton: bool) -> AutoloadEntry {
        AutoloadEntry {
            name: name.to_string(),
            path: path.to_string(),
            is_singleton: singleton,
        }
    }

    #[test]
    fn base_class_defaults_to_node_without_extends() {
        assert_eq!(base_class_from_source("var x = 1\n"), "Node");
        assert_eq!(base_class_from_source(""), "Node");
    }

    #[test]
    fn base_class_reads_extends() {
        assert_eq!(base_class_from_source("extends Node2D\n"), "Node2D");
        assert_eq!(
            base_class_from_source("# comment\nextends Control\nvar x = 1"),
            "Control"
        );
        assert_eq!(
            base_class_from_source("class_name Foo\nextends CanvasLayer"),
            "CanvasLayer"
        );
    }

    #[test]
    fn base_class_path_extends_falls_back_to_node() {
        assert_eq!(
            base_class_from_source("extends \"res://base.gd\"\n"),
            "Node"
        );
    }

    #[test]
    fn script_autoload_instantiated_under_root_with_ready_side_effect() {
        let dir = unique_temp_dir("script");
        std::fs::write(
            dir.join("global.gd"),
            "extends Node\nvar loaded = false\nfunc _ready():\n    loaded = true\n",
        )
        .unwrap();

        let mut tree = SceneTree::new();
        let root_id = tree.root_id();
        let autoloads = [entry("Global", "res://global.gd", true)];
        let result = instantiate_autoloads(&mut tree, root_id, &autoloads, &dir);

        assert_eq!(result.len(), 1);
        let gid = result.node_id("Global").expect("Global instantiated");

        // Node exists under root with the autoload name.
        let node = tree.get_node(gid).unwrap();
        assert_eq!(node.name(), "Global");
        assert!(tree.get_script(gid).is_some(), "script attached");

        // Before lifecycle, `loaded` is still its default (false).
        // After enter_tree/_ready, it flips to true.
        LifecycleManager::enter_tree(&mut tree, gid);
        let script = tree.get_script(gid).unwrap();
        let loaded = script
            .list_properties()
            .into_iter()
            .find(|p| p.name == "loaded")
            .map(|p| p.default_value)
            .expect("loaded var present");
        assert_eq!(loaded, Variant::Bool(true), "_ready mutated loaded");
    }

    #[test]
    fn boot_autoloads_from_dir_reads_project_godot() {
        let dir = unique_temp_dir("boot");
        std::fs::write(
            dir.join("project.godot"),
            "[autoload]\nGlobal=\"*res://global.gd\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("global.gd"),
            "extends Node\nvar hp = 0\nfunc _ready():\n    hp = 42\n",
        )
        .unwrap();

        let mut tree = SceneTree::new();
        let result = boot_autoloads_from_dir(&mut tree, &dir);
        assert_eq!(result.len(), 1);

        let gid = tree
            .get_node_by_path("/root/Global")
            .expect("/root/Global exists");
        let hp = tree
            .get_script(gid)
            .unwrap()
            .list_properties()
            .into_iter()
            .find(|p| p.name == "hp")
            .map(|p| p.default_value);
        assert_eq!(hp, Some(Variant::Int(42)), "_ready ran during boot");
    }

    #[test]
    fn scene_autoload_instantiated_and_renamed() {
        let dir = unique_temp_dir("scene");
        // Minimal scene whose root node is named "Root" — after instancing as
        // an autoload it must be renamed to the autoload name "Hud".
        std::fs::write(
            dir.join("hud.tscn"),
            "[gd_scene format=3]\n\n[node name=\"Root\" type=\"CanvasLayer\"]\n",
        )
        .unwrap();

        let mut tree = SceneTree::new();
        let root_id = tree.root_id();
        let autoloads = [entry("Hud", "res://hud.tscn", false)];
        let result = instantiate_autoloads(&mut tree, root_id, &autoloads, &dir);

        assert_eq!(result.len(), 1);
        let hid = result.node_id("Hud").expect("Hud instantiated");
        assert_eq!(tree.get_node(hid).unwrap().name(), "Hud");
        assert_eq!(tree.get_node(hid).unwrap().class_name(), "CanvasLayer");
    }

    #[test]
    fn no_project_file_yields_empty() {
        let dir = unique_temp_dir("empty");
        let mut tree = SceneTree::new();
        let result = boot_autoloads_from_dir(&mut tree, &dir);
        assert!(result.is_empty());
    }

    #[test]
    fn missing_script_file_is_skipped() {
        let dir = unique_temp_dir("missing");
        let mut tree = SceneTree::new();
        let root_id = tree.root_id();
        let autoloads = [entry("Gone", "res://does_not_exist.gd", true)];
        let result = instantiate_autoloads(&mut tree, root_id, &autoloads, &dir);
        assert!(result.is_empty(), "missing autoload skipped, not panicked");
    }
}
