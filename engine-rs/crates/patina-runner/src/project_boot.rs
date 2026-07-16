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

    // (5) Seed the runtime InputMap from `[input]`. Godot's
    // InputMap::load_from_project_settings installs the builtin `ui_*` actions
    // and then registers each project action with its real deadzone and bound
    // events (keycodes), so `Input.is_action_pressed` fires for project actions
    // after a matching key press. `ProjectLoader::get_input_map` keeps only
    // lossy action names (no deadzone, no bound events), so hand the raw
    // project.godot text to the full parser instead. `ProjectLoader::load`
    // above already proved the file reads; on the unexpected re-read failure we
    // fall back to just the builtin `ui_*` set.
    let input_map = match std::fs::read_to_string(project_root.join("project.godot")) {
        Ok(content) => InputMap::load_from_project_godot(&content),
        Err(_) => {
            let mut m = InputMap::new();
            m.install_builtin_ui_actions();
            m
        }
    };

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

#[cfg(test)]
mod tests {
    //! pat-nzgdu.3: the runner's project boot path must instantiate every
    //! `[autoload]` entry under `/root` before the main scene, so scene scripts
    //! can reference `/root/<Name>` in `_ready`. The shared `gdscene::autoload`
    //! helper is unit-tested on its own; these tests cover the patina-runner
    //! wiring (`boot_project`) end to end at the runner boot site.

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // `boot_project` mutates the process-global `res://` path roots via
    // `set_path_roots`, so serialize these tests to stay robust under
    // `cargo test` (nextest already isolates each test in its own process).
    static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn unique_temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "patina_nzgdu3_{}_{}_{}",
            tag,
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// pat-nzgdu.69: booting seeds the runtime InputMap from the project's
    /// `[input]` section — with the project's own deadzone and bound key events,
    /// not a hardcoded placeholder — plus the builtin `ui_*` action set, so
    /// `Input.is_action_pressed` can fire for a project-defined action after the
    /// matching key press. Proven end-to-end through `boot_project`, not by
    /// populating the map by hand.
    #[test]
    fn project_input_map_honored() {
        let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = unique_temp_dir("inputmap");
        std::fs::write(
            dir.join("project.godot"),
            "[application]\nconfig/name=\"InputMapBootTest\"\n\n\
             [input]\n\n\
             jump={\n\
             \"deadzone\": 0.35,\n\
             \"events\": [Object(InputEventKey,\"resource_local_to_scene\":false,\"device\":-1,\"keycode\":32,\"physical_keycode\":32,\"pressed\":false,\"script\":null)]\n\
             }\n",
        )
        .unwrap();

        let booted = boot_project(&dir).expect("boot project.godot with [input]");

        // The project action exists with the project's deadzone (0.35) — not the
        // old hardcoded 0.5.
        let actions: Vec<&str> = booted.input_map.actions().map(|s| s.as_str()).collect();
        assert!(
            actions.contains(&"jump"),
            "project action 'jump' registered at boot, got {actions:?}"
        );
        assert!(
            (booted.input_map.get_deadzone("jump") - 0.35).abs() < 1e-6,
            "action deadzone matches the project value, got {}",
            booted.input_map.get_deadzone("jump")
        );

        // The project's Space key event is bound to the action, so a Space press
        // triggers it (the runtime input path consults this map).
        let space = gdplatform::InputEvent::Key {
            key: gdplatform::Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        assert!(
            booted.input_map.event_matches_action(&space, "jump"),
            "a Space press triggers the project-defined 'jump' action"
        );

        // The builtin `ui_*` actions are also present (installed from the default
        // table, not per-demo).
        assert!(
            booted.input_map.actions().any(|a| a.as_str() == "ui_accept"),
            "builtin ui_* actions are registered at boot"
        );
    }

    #[test]
    fn boot_project_instantiates_script_autoloads_under_root() {
        let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = unique_temp_dir("script");
        std::fs::write(
            dir.join("project.godot"),
            "[application]\nconfig/name=\"AutoloadBootTest\"\n\n\
             [autoload]\nGlobal=\"*res://global.gd\"\nMusic=\"*res://music.gd\"\n",
        )
        .unwrap();
        // Script autoloads: `boot_project` stores the script path on a bare Node
        // and does not read the file, so the `.gd` files need not exist.

        let booted = boot_project(&dir).expect("boot project.godot with autoloads");

        // Both autoloads instantiated and tracked.
        assert_eq!(booted.autoload_roots.len(), 2, "two autoloads instantiated");
        let names: Vec<&str> = booted
            .autoload_roots
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        assert!(names.contains(&"Global"), "Global autoload present");
        assert!(names.contains(&"Music"), "Music autoload present");

        // Reachable as `/root/<Name>` — Godot's autoload access path.
        let global = booted
            .tree
            .get_node_by_path("/root/Global")
            .expect("/root/Global exists");
        assert_eq!(booted.tree.get_node(global).unwrap().name(), "Global");
        assert!(
            booted.tree.get_node_by_path("/root/Music").is_some(),
            "/root/Music exists"
        );

        // No main scene declared → none instanced; autoloads still present.
        assert!(booted.main_scene_root.is_none(), "no main scene declared");
    }

    #[test]
    fn boot_project_autoloads_coexist_with_main_scene() {
        let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = unique_temp_dir("mainscene");
        std::fs::write(
            dir.join("project.godot"),
            "[application]\nrun/main_scene=\"res://main.tscn\"\n\n\
             [autoload]\nGlobal=\"*res://global.gd\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("main.tscn"),
            "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n",
        )
        .unwrap();

        let booted = boot_project(&dir).expect("boot autoload + main scene");

        // The main scene instanced under `/root` ...
        let main_root = booted.main_scene_root.expect("main scene instanced");
        assert_eq!(booted.tree.get_node(main_root).unwrap().name(), "Main");

        // ... and the autoload constructed before it still lives under `/root`
        // (Godot builds autoloads first so the main scene's `_ready` sees them).
        assert_eq!(booted.autoload_roots.len(), 1, "autoload instantiated");
        let global = booted
            .tree
            .get_node_by_path("/root/Global")
            .expect("/root/Global survives main-scene instancing");
        assert_eq!(booted.tree.get_node(global).unwrap().name(), "Global");
    }
}
