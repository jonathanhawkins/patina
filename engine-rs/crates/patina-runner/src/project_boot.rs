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

use gdcore::file_access::FileAccess;
use gdcore::{set_path_roots, ProjectSettings};
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

/// Boots an exported game from a Godot `.pck`, the way Godot boots a shipped
/// build (pat-0q2kx.134).
///
/// The pack is mounted over `res://` before anything is parsed, so
/// `project.godot`, the main scene, the autoloads and every texture they
/// reference all resolve out of the archive — the loose project directory does
/// not need to exist. `res://` still points at the pack's own directory, so a
/// file deliberately shipped beside the pack keeps resolving.
///
/// `user_dir` maps `user://` exactly as in [`boot_project_with_user_root`].
pub fn boot_project_from_pck(
    pck_path: &Path,
    user_dir: Option<PathBuf>,
) -> Result<BootedProject, String> {
    let bytes = std::fs::read(pck_path)
        .map_err(|e| format!("failed to read pack '{}': {e}", pck_path.display()))?;
    let mount = gdresource::pck::PckMount::from_bytes(bytes)
        .map_err(|e| format!("'{}' is not a valid .pck: {e}", pck_path.display()))?;

    // `res://` falls back to the pack's directory for anything not packed.
    let fallback_root = pck_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    set_path_roots(Some(fallback_root.clone()), user_dir.clone());

    let label = pck_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game".to_string());
    gdcore::file_access::mount_pack(label, std::sync::Arc::new(mount), 0);

    let content = FileAccess::read_file_string("res://project.godot").map_err(|e| {
        format!(
            "pack '{}' has no readable res://project.godot: {e}",
            pck_path.display()
        )
    })?;
    boot_from_project_text(&fallback_root, &content, user_dir)
}

/// Like [`boot_project`], but with an explicit `user://` root (e.g. the platform
/// application-data directory). Passing `None` leaves `user://` unmapped.
pub fn boot_project_with_user_root(
    project_dir: &Path,
    user_dir: Option<PathBuf>,
) -> Result<BootedProject, String> {
    let project_root = project_dir.to_path_buf();
    let content = std::fs::read_to_string(project_root.join("project.godot")).map_err(|e| {
        format!(
            "failed to load project.godot from {}: {e}",
            project_root.display()
        )
    })?;
    // `res://` must resolve before the shared body loads scenes.
    set_path_roots(Some(project_root.clone()), user_dir.clone());
    boot_from_project_text(&project_root, &content, user_dir)
}

/// The boot body shared by the loose-directory and `.pck` entry points.
///
/// Everything from here on reads through `res://`, so it does not care whether
/// the bytes come from disk or from a mounted pack. `project_root` is only the
/// `res://` fallback and the path reported in messages; both callers have already
/// installed the path roots.
fn boot_from_project_text(
    project_root: &Path,
    content: &str,
    user_dir: Option<PathBuf>,
) -> Result<BootedProject, String> {
    let project_root = project_root.to_path_buf();

    // (1) Parse project.godot for scene/autoloads/input and for ProjectSettings.
    let loader = ProjectLoader::from_text(&project_root, content)
        .map_err(|e| format!("failed to parse project.godot: {e}"))?;
    let settings = ProjectSettings::from_godot_format(content)
        .map_err(|e| format!("failed to load ProjectSettings: {e}"))?;

    // (1a) Name the `user://` directory after the project, the way Godot does
    // (`<data dir>/Godot/app_userdata/<application/config/name>`). Without this
    // every Patina project shared one `user://` (pat-0q2kx.123). The declared
    // feature tags go in at the same time so `OS.has_feature("my_tag")` answers
    // for project-defined tags, not just the built-in ones.
    gdplatform::os::set_project_name(settings.project_name());
    gdplatform::os::clear_custom_features();
    if let Some(features) = settings.get("application/config/features") {
        if let Some(list) = features.as_string() {
            for tag in list.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                gdplatform::os::add_custom_feature(tag);
            }
        }
    }

    // (1a2) Publish the parsed settings to the script-visible `ProjectSettings`
    // singleton (pat-0q2kx.135). The interpreter's store was seeded by nothing,
    // so `ProjectSettings.get_setting("physics/2d/default_gravity")` answered
    // Nil inside a project that plainly declares it. Feature tags come from the
    // same OS layer that answers `OS.has_feature`, so `setting.web` resolves
    // against the same truth, and the source path is remembered so
    // `ProjectSettings.save()` writes back where it read from.
    gdscript_interop::interpreter::seed_project_settings(
        settings
            .iter()
            .filter_map(|(k, v)| settings_value_to_variant(v).map(|var| (k.to_string(), var))),
    );
    gdscript_interop::interpreter::set_project_feature_tags(active_feature_tags());
    gdscript_interop::interpreter::set_project_settings_path(
        project_root
            .join("project.godot")
            .to_string_lossy()
            .to_string(),
    );

    // (1b) Install the DisplayServer backend now that the project's configured
    // viewport is known. Without this a script calling
    // `DisplayServer.window_get_size()` got a hardcoded 1920x1080 from inside
    // the interpreter, unrelated to `display/window/size/viewport_*` and to the
    // window it was actually running in (pat-0q2kx.38).
    gdcore::display_backend::install_display_backend(std::sync::Arc::new(
        gdcore::display_backend::HeadlessDisplayBackend::new(
            settings.viewport_width(),
            settings.viewport_height(),
        ),
    ));

    // (2) Re-assert res:// / user:// roots. Both entry points set them before
    // calling in (the `.pck` path must, to read project.godot at all); doing it
    // again here keeps `user://` correct now that the project name is known.
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
    // lossy action names (no deadzone, no bound events), so the raw
    // project.godot text goes to the full parser instead. That text is already
    // in hand, which also removes the old re-read-from-disk fallback and is what
    // lets this work for a packed project (pat-0q2kx.134).
    let input_map = InputMap::load_from_project_godot(content);

    Ok(BootedProject {
        tree,
        settings,
        input_map,
        main_scene_root,
        autoload_roots,
    })
}

/// Maps a `project.godot` value to the `Variant` a script sees
/// (pat-0q2kx.135). Every `SettingsValue` has a Variant counterpart, so this is
/// total — there is no silently dropped setting.
fn settings_value_to_variant(v: &gdcore::SettingsValue) -> Option<Variant> {
    use gdcore::SettingsValue as S;
    Some(match v {
        S::Bool(b) => Variant::Bool(*b),
        S::Int(i) => Variant::Int(*i),
        S::Float(f) => Variant::Float(*f),
        S::String(s) => Variant::String(s.clone()),
        S::Vector2(x, y) => Variant::Vector2(gdcore::math::Vector2::new(*x as f32, *y as f32)),
        S::Vector3(x, y, z) => {
            Variant::Vector3(gdcore::math::Vector3::new(*x as f32, *y as f32, *z as f32))
        }
        S::Color(r, g, b, a) => Variant::Color(gdcore::math::Color::new(
            *r as f32, *g as f32, *b as f32, *a as f32,
        )),
    })
}

/// The feature tags active for this run, in the order
/// `get_setting_with_override` should try them.
///
/// Sourced from `gdplatform::os::has_feature` so a script's
/// `OS.has_feature("web")` and a `setting.web` override can never disagree —
/// two tables answering the same question is the bug pat-0q2kx.123 removed.
fn active_feature_tags() -> Vec<String> {
    [
        "web", "editor", "template", "debug", "release", "pc", "desktop", "mobile", "linuxbsd",
        "macos", "windows", "linux", "android", "ios", "x86_64", "arm64",
    ]
    .into_iter()
    .filter(|tag| gdplatform::os::has_feature(tag))
    .map(str::to_string)
    .collect()
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
    // Pack-aware: `FileAccess::read_file_string` serves a mounted `.pck` before
    // disk, so a packed and a loose project take the identical load path
    // (pat-0q2kx.134). The `res://` path is used in errors rather than a
    // filesystem path, because a packed read has none.
    let source = FileAccess::read_file_string(res_path)
        .map_err(|e| format!("failed to read scene '{res_path}': {e}"))?;
    let packed = PackedScene::from_tscn(&source)
        .map_err(|e| format!("failed to parse scene '{res_path}': {e}"))?;
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
            booted
                .input_map
                .actions()
                .any(|a| a.as_str() == "ui_accept"),
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
