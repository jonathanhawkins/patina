//! pat-mgixk.414 acceptance — booting a project directory attaches the main
//! scene's GDScripts.
//!
//! Godot's `godot --path <dir>` instances `application/run/main_scene` with
//! every node's attached script, then runs `_enter_tree` / `_ready` and the
//! frame loop (`main/main.cpp`).
//!
//! Patina's project-directory boot did the first half only. `boot_project`
//! instanced the main scene and the autoloads, but **only script autoloads ever
//! got a script** — so every node in the main scene ran as if it had none: no
//! `_ready`, no `_process`, no prints. Exactly one of the three boot paths (the
//! single-`.tscn` one in `main.rs`) remembered to attach, which is why the gap
//! survived: whichever path you happened to test, the other two were broken.
//!
//! The conformance gate measured it as **0 of 4 projects passing, with zero
//! `[T]` lines from any of them**.
//!
//! # Why this asserts the ATTACH and the `_ready`, separately
//!
//! ⚠️ The two failures look identical from a property read. A script that was
//! never attached and a script that was attached but never ran both leave the
//! property at its declared default. So the first assertion is
//! `get_script(..).is_some()` — did the node get a script at all, which is the
//! actual bug — and only then the property, which is whether it ran.

use std::path::PathBuf;

use gdscene::{LifecycleManager, NodeId, SceneTree};
use gdvariant::Variant;
use patina_runner::project_boot::{attach_scripts, boot_project};

fn fixture_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/boot_attaches_scripts"
    ))
}

/// Boots the fixture project exactly as `patina-runner <project_dir>` does, then
/// runs `enter_tree` so `_ready` fires.
fn boot() -> (SceneTree, NodeId) {
    let booted = boot_project(&fixture_dir()).expect("boot from project.godot");
    let mut tree = booted.tree;
    let main_root = booted
        .main_scene_root
        .expect("the project declares run/main_scene, so it must instance");
    LifecycleManager::enter_tree(&mut tree, main_root);
    (tree, main_root)
}

fn node_by_name(tree: &SceneTree, name: &str) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|&id| tree.get_node(id).map(|n| n.name()) == Some(name))
        .unwrap_or_else(|| panic!("`{name}` must exist in the booted tree"))
}

fn script_var(tree: &SceneTree, id: NodeId, name: &str) -> Variant {
    tree.get_script(id)
        .unwrap_or_else(|| panic!("node must have a script to read `{name}` from"))
        .get_property(name)
        .unwrap_or_else(|| panic!("script must declare `{name}`"))
}

/// **The acceptance test.**
#[test]
fn project_boot_attaches_main_scene_scripts() {
    let (tree, main_root) = boot();

    // -- the bug itself: was a script attached at all? ----------------------
    assert!(
        tree.get_script(main_root).is_some(),
        "booting a project directory must attach the main scene's `_script_path` \
         GDScript. None attached is the pat-mgixk.414 bug: `boot_project` \
         instanced the scene and only ever scripted AUTOLOADS, so the main \
         scene ran scriptless."
    );

    // -- and did it run? ----------------------------------------------------
    assert_eq!(
        script_var(&tree, main_root, "ready_ran"),
        Variant::Bool(true),
        "`_ready` must have run on the attached script. False with a script \
         present would mean the attach landed too late for the lifecycle — the \
         attach has to happen before any caller runs `enter_tree`."
    );

    // The script read a sibling through `get_node`, so this also proves the
    // instance was wired to the tree rather than attached in isolation.
    assert_eq!(
        script_var(&tree, main_root, "plain_name"),
        Variant::String("Plain".to_string()),
        "`_ready` must have had scene access; an empty name means the script \
         ran without being able to reach its own siblings"
    );

    // -- a node with no `_script_path` is left alone ------------------------
    let plain = node_by_name(&tree, "Plain");
    assert!(
        tree.get_script(plain).is_none(),
        "a node that declares no `_script_path` must not be given a script"
    );
}

/// Re-attaching must SKIP nodes that already have a script.
///
/// ⚠️ This is what protects autoloads. They are instantiated with their script
/// before the main scene loads, and their `_ready` has already run by the time
/// the main scene is attached. A re-attach would re-parse the source and hand
/// back a fresh instance with every variable at its declared default, silently
/// discarding whatever that `_ready` produced.
///
/// Asserting it needs no setter: `ready_ran` is `true` only because `_ready`
/// ran. A clobbering re-attach would replace the instance with a fresh one whose
/// `ready_ran` is still `false`, since `_ready` does not run again here.
#[test]
fn attaching_twice_does_not_clobber_a_running_script() {
    let (mut tree, main_root) = boot();
    assert_eq!(
        script_var(&tree, main_root, "ready_ran"),
        Variant::Bool(true),
        "precondition: the first boot ran `_ready`"
    );

    let reattached = attach_scripts(&mut tree, &fixture_dir());

    assert_eq!(
        reattached, 0,
        "every node already has its script, so a second pass must attach none"
    );
    assert_eq!(
        script_var(&tree, main_root, "ready_ran"),
        Variant::Bool(true),
        "the already-running script must survive a second attach pass. False \
         means it was re-parsed and replaced, which is how an autoload would \
         lose the state its `_ready` had already produced."
    );
}
