//! Shortcut matching for the `_shortcut_input` stage (pat-s3rrv.175).
//!
//! Godot's `Shortcut` is a Resource holding an `Array[InputEvent]`, and
//! `BaseButton` fires `pressed` when any of those events matches during the
//! shortcut stage (upstream/godot/scene/gui/base_button.cpp `shortcut_input`).
//!
//! Patina has no shared resource object during scene instancing — inline
//! resources are flattened onto the node — so the Shortcut's `events` array is
//! unreachable once loading finishes. `PackedScene::from_tscn` therefore
//! flattens each `shortcut = SubResource("…")` into a `__shortcut__events`
//! companion property holding `[keycode, modifier_mask, …]`, and this module is
//! the reader for it. That keeps input-time matching a pure tree walk: the
//! scene object is long gone by the time a key arrives.

use crate::node::NodeId;
use crate::scene_tree::SceneTree;
use gdplatform::input::InputEvent;
use gdvariant::Variant;

/// Companion listing the input ACTIONS a node's Shortcut fires on — its
/// `InputEventAction` events — written by `PackedScene::from_tscn` beside
/// `__shortcut__events`.
pub const SHORTCUT_ACTIONS_KEY: &str = "__shortcut__actions";

/// Modifier bits packed alongside each keycode. Must stay in step with
/// `packed_scene::shortcut_event_pairs`, which writes them.
pub const CTRL_BIT: i32 = 1;
/// Shift modifier bit.
pub const SHIFT_BIT: i32 = 2;
/// Alt modifier bit.
pub const ALT_BIT: i32 = 4;

/// The `(keycode, modifier_mask)` a key event presents to a shortcut, or `None`
/// if the event cannot trigger one.
///
/// Releases are excluded: Godot fires a shortcut on the press, and accepting
/// both edges would fire every accelerator twice.
fn event_accelerator(event: &InputEvent) -> Option<(i32, i32)> {
    let InputEvent::Key {
        key,
        pressed,
        shift,
        ctrl,
        alt,
    } = event
    else {
        return None;
    };
    if !*pressed {
        return None;
    }
    let keycode = i32::try_from(key.to_godot_keycode()).ok()?;
    let mut mods = 0;
    if *ctrl {
        mods |= CTRL_BIT;
    }
    if *shift {
        mods |= SHIFT_BIT;
    }
    if *alt {
        mods |= ALT_BIT;
    }
    Some((keycode, mods))
}

/// Whether `node_id` declares a shortcut matching this accelerator.
///
/// The modifier mask is compared for **equality**, not containment: Godot
/// matches shortcuts exactly, so Ctrl+Shift+S must not trigger a Ctrl+S
/// shortcut. A containment test would make every accelerator fire under any
/// extra modifier the user happened to be holding.
fn node_matches(
    tree: &SceneTree,
    node_id: NodeId,
    event: &InputEvent,
    keycode: i32,
    mods: i32,
) -> bool {
    let Some(node) = tree.get_node(node_id) else {
        return false;
    };
    let has_keys = matches!(node.get_property("__shortcut__events"), Variant::PackedInt32Array(_));
    let has_actions = matches!(
        node.get_property(SHORTCUT_ACTIONS_KEY),
        Variant::PackedStringArray(_)
    );
    if !has_keys && !has_actions {
        return false;
    }
    // Godot's BaseButton returns early when disabled, so a greyed-out button
    // does not respond to its own accelerator.
    if matches!(node.get_property("disabled"), Variant::Bool(true)) {
        return false;
    }
    // ⚠️ …and when it is not visible in the tree
    // (`BaseButton::shortcut_input`: `is_visible_in_tree()`). Without this a
    // HIDDEN button kept answering its accelerator: Dodge the Creeps hides its
    // Start button during play, and Space would have restarted the run.
    if !crate::node2d::is_visible_in_tree(tree, node_id) {
        return false;
    }
    if let Variant::PackedInt32Array(pairs) = node.get_property("__shortcut__events") {
        if pairs
            .chunks_exact(2)
            .any(|pair| pair[0] == keycode && pair[1] == mods)
        {
            return true;
        }
    }
    // An `InputEventAction` shortcut matches whatever presses that action,
    // through the project's own bindings (`InputEventAction::is_match`). The
    // match reuses `InputMap`'s rules on the tree's mirror of those bindings,
    // so a shortcut and `Input.is_action_pressed` can never disagree.
    if let Variant::PackedStringArray(actions) = node.get_property(SHORTCUT_ACTIONS_KEY) {
        for action in actions.iter() {
            let bindings = tree.input_map_action_bindings(action);
            if bindings.is_empty() {
                continue;
            }
            let mut map = gdplatform::input::InputMap::new();
            map.add_action(action, 0.5);
            for binding in bindings {
                map.action_add_event(action, binding.clone());
            }
            if map.event_matches_action(event, action) {
                return true;
            }
        }
    }
    false
}

/// Fires the first node whose shortcut matches `event`, emitting `pressed` and
/// marking the event handled. Returns the node that fired, if any.
///
/// Walks in **reverse** tree order to match how `SceneTree::push_input`
/// dispatches every other input stage: the deepest, latest node gets first
/// refusal over its ancestors.
pub fn dispatch_shortcuts(tree: &mut SceneTree, event: &InputEvent) -> Option<NodeId> {
    let (keycode, mods) = event_accelerator(event)?;
    let target = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .rev()
        .find(|&id| node_matches(tree, id, event, keycode, mods))?;
    tree.emit_signal(target, "pressed", &[]);
    tree.set_input_as_handled();
    Some(target)
}
