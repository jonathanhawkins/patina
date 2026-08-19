//! pat-s3rrv.14 item (c) — `InputMap.load_from_project_settings()` restores the
//! project's map and DISCARDS runtime changes.
//!
//! # What was wrong
//!
//! The arm returned `Nil` with the comment "Patina's map is installed by the
//! host at boot, so the script-visible effect is already in place". That is true
//! of the boot-time map and silently drops the half that matters: in Godot the
//! call **rebuilds** the map from ProjectSettings, so every runtime
//! `add_action` / `action_add_event` / deadzone change since boot is undone.
//!
//! The user-visible shape of the gap is a rebinding screen. "Restore defaults"
//! IS this call — and a no-op leaves every custom binding exactly where it was,
//! which is the one outcome the player asked not to have.
//!
//! ⚠️ A test that only asserted the project action survives would pass against
//! the no-op, because the project action was never removed. The load-bearing
//! assertion is that the *runtime* action is GONE, and it is only meaningful
//! next to a control proving that action existed first.

use gdplatform::input::{ActionBinding, InputMap, Key};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Frame 1 adds a runtime action, frame 2 confirms it took and asks for the
/// reload, frame 3 observes the result. Split across frames because `InputMap`
/// mutators are queued for the frame owner — the engine map belongs to
/// `MainLoop`, not to the script.
const REBINDER: &str = "\
extends Node
var frame = 0
var had_temp_before = false
var has_temp_after = true
var has_fire_after = false
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        InputMap.add_action(\"temp\", 0.5)
    elif self.frame == 2:
        self.had_temp_before = InputMap.has_action(\"temp\")
        InputMap.load_from_project_settings()
    elif self.frame == 3:
        self.has_temp_after = InputMap.has_action(\"temp\")
        self.has_fire_after = InputMap.has_action(\"fire\")
";

fn var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

#[test]
fn load_from_project_settings_discards_runtime_actions_and_restores_the_project_map() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Rebinder", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(REBINDER, node).expect("script parses")),
    );

    // The "project" map a host installs at boot.
    let mut map = InputMap::new();
    map.add_action("fire", 0.5);
    map.action_add_event("fire", ActionBinding::KeyBinding(Key::F));

    let mut ml = MainLoop::new(tree);
    ml.set_input_map(map);

    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    // The control. Without this the assertion below is vacuous: an action that
    // never existed is also "gone" after a reload.
    assert_eq!(
        var(&ml, node, "had_temp_before"),
        Variant::Bool(true),
        "the runtime `add_action` must have taken effect before the reload, or \
         the reset assertion proves nothing"
    );

    // The headline: the runtime action is gone.
    assert_eq!(
        var(&ml, node, "has_temp_after"),
        Variant::Bool(false),
        "`load_from_project_settings()` must DISCARD runtime actions. A `true` \
         here is the old no-op: the call returned Nil and changed nothing"
    );

    // ...and the reload restored rather than wiped: the project action is back.
    assert_eq!(
        var(&ml, node, "has_fire_after"),
        Variant::Bool(true),
        "the project-declared action must survive the reload — clearing the map \
         instead of replacing it would also pass the assertion above"
    );
}

/// With no host-installed map there is nothing to return to, and the call must
/// be inert rather than clearing whatever a script built up.
///
/// This is the case a `MainLoop` built directly in a test hits, and getting it
/// wrong would make the reload a data-loss bug in exactly the setups that never
/// booted a project.
#[test]
fn a_reload_without_a_project_map_is_inert() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Rebinder", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(REBINDER, node).expect("script parses")),
    );

    // NOTE: no `set_input_map` — nothing was ever installed.
    let mut ml = MainLoop::new(tree);
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        var(&ml, node, "had_temp_before"),
        Variant::Bool(true),
        "the runtime action still registers without a project map"
    );
    assert_eq!(
        var(&ml, node, "has_temp_after"),
        Variant::Bool(true),
        "with no baseline to restore, the reload must leave the map alone \
         rather than clearing it"
    );
}
