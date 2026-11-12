//! pat-nzgdu.43: GUI Range value-model acceptance test.
//!
//! Godot's shared `Range` model (`Range::set_value`) clamps `value` to
//! `[min, max - page]` and snaps it to `step`; a slider maps a grabber-drag
//! position to a ratio in `[0, 1]`, and a `ProgressBar` fills its rect by that
//! ratio. HSlider / VSlider / ScrollBar / SpinBox / ProgressBar all derive this
//! shared model, so exercising it via HSlider + ProgressBar covers the parity
//! behaviour the audit named (snap-to-step, clamp, value_changed, fill ratio).

use gdscene::control::{get_range_ratio, get_range_value, set_range_value};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

fn add_control(tree: &mut SceneTree, name: &str, class: &str) -> NodeId {
    let root = tree.root_id();
    let node = Node::new(name, class);
    tree.add_child(root, node).unwrap()
}

fn set_f64(tree: &mut SceneTree, id: NodeId, key: &str, v: f64) {
    tree.get_node_mut(id)
        .unwrap()
        .set_property(key, Variant::Float(v));
}

#[test]
fn range_slider_spinbox_value_changed() {
    let mut tree = SceneTree::new();

    // --- HSlider with min=0, max=10, step=2 ---
    let slider = add_control(&mut tree, "S", "HSlider");
    set_f64(&mut tree, slider, "min_value", 0.0);
    set_f64(&mut tree, slider, "max_value", 10.0);
    set_f64(&mut tree, slider, "step", 2.0);

    // Dragging the grabber to 55% maps to ratio 0.55 -> raw value 5.5, which the
    // Range model snaps to the nearest multiple of step (2) -> 6. set_range_value
    // returning true is precisely the condition under which `value_changed(6)`
    // fires (see the Range::set_value contract in control.rs).
    let (min, max) = (0.0_f64, 10.0_f64);
    let dragged_to_55_percent = min + 0.55 * (max - min); // 5.5
    let changed = set_range_value(&mut tree, slider, dragged_to_55_percent);
    assert!(
        changed,
        "0 -> 6 must report a change (value_changed emitted)"
    );
    assert_eq!(
        get_range_value(&tree, slider),
        6.0,
        "dragging to 55% with step 2 snaps the value to 6"
    );

    // Setting a value beyond the maximum clamps to max.
    set_range_value(&mut tree, slider, 999.0);
    assert_eq!(
        get_range_value(&tree, slider),
        10.0,
        "value=999 clamps to max 10"
    );
    // Already at the max: re-setting reports no change (no spurious value_changed).
    assert!(
        !set_range_value(&mut tree, slider, 999.0),
        "no change when the value is already clamped at the max"
    );

    // --- ProgressBar with value=5, max=10 -> fill covers half its rect ---
    let bar = add_control(&mut tree, "P", "ProgressBar");
    set_f64(&mut tree, bar, "min_value", 0.0);
    set_f64(&mut tree, bar, "max_value", 10.0);
    set_f64(&mut tree, bar, "step", 1.0);
    set_range_value(&mut tree, bar, 5.0);
    assert_eq!(get_range_value(&tree, bar), 5.0);
    let ratio = get_range_ratio(&tree, bar);
    assert!(
        (ratio - 0.5).abs() < 1e-4,
        "a ProgressBar at value 5 / max 10 fills half its rect (ratio 0.5), got {ratio}"
    );
}
