//! pat-s3rrv.161 acceptance: buttons sharing a `ButtonGroup` behave as radio
//! buttons, and `allow_unpress` decides whether the active one can be clicked
//! off.
//!
//! Every assertion drives the **real input path** —
//! `SceneTree::push_input` -> `gui_input_stage` -> property write + signal emit
//! — never `set_button_pressed` directly, so a regression that leaves the
//! helpers correct but unreachable from a click still fails here.
//!
//! # What a Patina "group" is
//!
//! Godot compares `Ref<ButtonGroup>` pointers. Patina resources are values with
//! no object identity, so a group is the **identity string** every member names
//! in `button_group` — two buttons naming the same `SubResource("…")` are in the
//! same group. That is the same equivalence Godot's pointer comparison
//! produces for a scene-authored group, reached differently.
//!
//! # Two things this file deliberately does NOT assert
//!
//! 1. **`ButtonGroup.pressed(button)` is not emitted.** The signal belongs to
//!    the *resource*, and a Patina resource has no object to emit from or to
//!    connect to — `BaseButton.button_group` reads as `Nil`, so no script could
//!    subscribe even if it were emitted. The observable behaviour is asserted
//!    through `button_group_pressed_button` instead, which is the same fact the
//!    signal would carry. Emitting into the void would be untestable and
//!    unreachable.
//! 2. **Clicking is not driven from a `.tscn`.** The click tests build their
//!    buttons by hand and set the flattened `__button_group__allow_unpress`
//!    companion directly; the `.tscn` tests at the bottom of this file assert
//!    that the loader produces that companion from the `[sub_resource]` body.
//!    Splitting them that way keeps a button-group failure from being reported
//!    as a Control layout failure, since driving a click at a scene-loaded
//!    Control would also exercise layout resolution.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::control::{
    button_group_allows_unpress, button_group_buttons, button_group_of,
    button_group_pressed_button, buttons_in_group,
};
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// What a `.tscn` leaves in `button_group` for `[sub_resource type="ButtonGroup"
/// id="Group_1"]`.
const GROUP: &str = "SubResource(\"Group_1\")";

/// The companion the loader flattens a `ButtonGroup` body onto its members as.
const ALLOW_UNPRESS: &str = "__button_group__allow_unpress";

fn button_at(tree: &mut SceneTree, parent: NodeId, name: &str, y: f32) -> NodeId {
    let id = tree.add_child(parent, Node::new(name, "CheckBox")).unwrap();
    if let Some(n) = tree.get_node_mut(id) {
        n.set_property("position", Variant::Vector2(Vector2::new(0.0, y)));
        n.set_property("size", Variant::Vector2(Vector2::new(80.0, 40.0)));
        n.set_property("toggle_mode", Variant::Bool(true));
        n.set_property("button_group", Variant::String(GROUP.to_owned()));
    }
    id
}

fn click(tree: &mut SceneTree, at: Vector2) {
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: at,
    });
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: at,
    });
}

fn is_pressed(tree: &SceneTree, node: NodeId) -> bool {
    tree.get_node(node)
        .map(|n| matches!(n.get_property("button_pressed"), Variant::Bool(true)))
        .unwrap_or(false)
}

/// Three grouped `CheckBox`es at y = 0, 100, 200; click them at y+20.
fn three_grouped() -> (SceneTree, NodeId, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = button_at(&mut tree, root, "A", 0.0);
    let b = button_at(&mut tree, root, "B", 100.0);
    let c = button_at(&mut tree, root, "C", 200.0);
    (tree, a, b, c)
}

fn click_a(tree: &mut SceneTree) {
    click(tree, Vector2::new(20.0, 20.0));
}
fn click_b(tree: &mut SceneTree) {
    click(tree, Vector2::new(20.0, 120.0));
}

/// 🔑 THE ACCEPTANCE.
#[test]
fn button_group_enforces_radio_exclusivity() {
    let (mut tree, a, b, c) = three_grouped();

    // -- membership -------------------------------------------------------
    // `get_buttons()` includes the receiver; Godot's `_get_buttons()` walks the
    // whole list with no `continue`, unlike `_unpress_group`.
    assert_eq!(
        button_group_buttons(&tree, a),
        vec![a, b, c],
        "get_buttons() must report all three members in tree order, including \
         the button it was reached through"
    );
    assert_eq!(
        button_group_pressed_button(&tree, a),
        None,
        "nothing is pressed before the first click"
    );

    // -- clicking A ------------------------------------------------------
    click_a(&mut tree);
    assert!(is_pressed(&tree, a), "clicking A must press it");
    assert!(
        !is_pressed(&tree, b) && !is_pressed(&tree, c),
        "and must leave B and C off — a group is mutually exclusive"
    );
    assert_eq!(
        button_group_pressed_button(&tree, a),
        Some(a),
        "get_pressed_button() must report A, the button the group would have \
         announced with ButtonGroup.pressed(A)"
    );

    // -- clicking B releases A --------------------------------------------
    click_b(&mut tree);
    assert!(is_pressed(&tree, b), "clicking B must press it");
    assert!(
        !is_pressed(&tree, a),
        "and must release A — this is the whole point of a group"
    );
    assert!(!is_pressed(&tree, c), "C is untouched");
    assert_eq!(
        button_group_pressed_button(&tree, a),
        Some(b),
        "the group's pressed button must follow the click, whichever member it \
         is asked through"
    );

    // -- clicking B again is refused (allow_unpress defaults to false) -----
    click_b(&mut tree);
    assert!(
        is_pressed(&tree, b),
        "re-clicking the active member of a group must NOT un-press it: with \
         allow_unpress false, Godot forces `status.pressed = true` back on \
         (base_button.cpp:43-46). A group that can be emptied by clicking is a \
         set of checkboxes, not radio buttons"
    );
    assert_eq!(
        button_group_pressed_button(&tree, a),
        Some(b),
        "the group must still report a pressed button"
    );
}

/// The other half of the same rule: with `allow_unpress` the active member
/// **does** turn off, leaving the group empty.
///
/// ⚠️ Paired with the refusal above on purpose. A build that ignored
/// `allow_unpress` entirely would pass the acceptance test — refusing is its
/// default — so only this test can tell "implements the flag" from "hardcodes
/// the default".
#[test]
fn allow_unpress_lets_the_active_member_turn_itself_off() {
    let (mut tree, a, b, _c) = three_grouped();
    for id in [a, b] {
        if let Some(n) = tree.get_node_mut(id) {
            n.set_property(ALLOW_UNPRESS, Variant::Bool(true));
        }
    }

    click_b(&mut tree);
    assert!(is_pressed(&tree, b), "first click presses B");

    click_b(&mut tree);
    assert!(
        !is_pressed(&tree, b),
        "with allow_unpress the active member must turn off when clicked again"
    );
    assert_eq!(
        button_group_pressed_button(&tree, a),
        None,
        "and the group is then legitimately empty — that is what allow_unpress \
         permits"
    );
}

/// Exclusivity must come from the group, not from proximity: two ungrouped
/// buttons are independent, and a button in a *different* group is untouched.
///
/// ⚠️ Without this, an implementation that un-pressed every toggle button in
/// the scene would pass every assertion above.
#[test]
fn a_group_does_not_reach_outside_itself() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = button_at(&mut tree, root, "A", 0.0);
    let b = button_at(&mut tree, root, "B", 100.0);
    let other = button_at(&mut tree, root, "Other", 200.0);
    if let Some(n) = tree.get_node_mut(other) {
        n.set_property(
            "button_group",
            Variant::String("SubResource(\"Group_2\")".to_owned()),
        );
    }
    let loner = button_at(&mut tree, root, "Loner", 300.0);
    if let Some(n) = tree.get_node_mut(loner) {
        n.set_property("button_group", Variant::String(String::new()));
    }

    assert_eq!(
        button_group_buttons(&tree, a),
        vec![a, b],
        "only the two members of Group_1 are in A's group"
    );
    assert_eq!(
        button_group_buttons(&tree, loner),
        Vec::<NodeId>::new(),
        "an ungrouped button has no group — it must NOT collect every other \
         ungrouped button, which is what an empty-string match would do"
    );

    // Turn the outsiders on, then click inside the group.
    click(&mut tree, Vector2::new(20.0, 220.0)); // Other
    click(&mut tree, Vector2::new(20.0, 320.0)); // Loner
    assert!(is_pressed(&tree, other) && is_pressed(&tree, loner));

    click_a(&mut tree);
    assert!(is_pressed(&tree, a), "A is pressed");
    assert!(
        is_pressed(&tree, other),
        "a button in a DIFFERENT group must be untouched"
    );
    assert!(
        is_pressed(&tree, loner),
        "an ungrouped button must be untouched"
    );
}

/// The group key is the resource reference itself, so membership survives
/// however the loader spelled it.
#[test]
fn membership_is_keyed_on_the_shared_resource_reference() {
    let (tree, a, b, c) = three_grouped();

    assert_eq!(
        button_group_of(&tree, a),
        GROUP,
        "the group key is the resource reference the scene wrote"
    );
    assert_eq!(
        buttons_in_group(&tree, GROUP),
        vec![a, b, c],
        "looking the group up by key must find the same members as looking it \
         up through a member"
    );
    assert_eq!(
        buttons_in_group(&tree, ""),
        Vec::<NodeId>::new(),
        "the empty key must match nothing — if it matched, every ungrouped \
         button in a scene would become one giant radio group"
    );
}

// ---------------------------------------------------------------------------
// The `.tscn` half: the group's BODY has to reach the member nodes
// ---------------------------------------------------------------------------

/// Two grouped CheckBoxes and an inline `ButtonGroup` that allows unpressing.
///
/// This is the shape the Godot editor writes: the flag lives in the
/// `[sub_resource]` body, and each node carries only a reference to it.
const SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="ButtonGroup" id="Group_1"]
allow_unpress = true

[node name="Root" type="Control"]

[node name="A" type="CheckBox" parent="."]
toggle_mode = true
button_group = SubResource("Group_1")

[node name="B" type="CheckBox" parent="."]
toggle_mode = true
button_group = SubResource("Group_1")
"#;

/// The same scene with no `allow_unpress` in the body, which is how the vast
/// majority of real groups are authored.
const SCENE_DEFAULT: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="ButtonGroup" id="Group_1"]

[node name="Root" type="Control"]

[node name="A" type="CheckBox" parent="."]
toggle_mode = true
button_group = SubResource("Group_1")

[node name="B" type="CheckBox" parent="."]
toggle_mode = true
button_group = SubResource("Group_1")
"#;

fn instance(src: &str) -> (SceneTree, NodeId, NodeId) {
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let find = |tree: &SceneTree, name: &str| {
        tree.all_nodes_in_tree_order()
            .into_iter()
            .find(|id| {
                tree.get_node(*id)
                    .map(|n| n.name() == name)
                    .unwrap_or(false)
            })
            .unwrap_or_else(|| panic!("no node named {name}"))
    };
    let a = find(&tree, "A");
    let b = find(&tree, "B");
    (tree, a, b)
}

/// 🔑 `allow_unpress` survives the trip from the `.tscn` to the node.
///
/// ⚠️ This is the half that was missing, and it failed *silently*: membership
/// is keyed on the reference text, which reaches the node on its own, so
/// exclusivity worked and only the flag was lost. A group authored with
/// `allow_unpress = true` behaved exactly like one without it, with nothing to
/// see in a log.
///
/// ⚠️ Asserted at the flatten rather than by clicking. The click path is proven
/// by the hand-built tests above; what a `.tscn` adds is precisely the question
/// of whether the sub-resource BODY reaches the member nodes, and testing that
/// through a click would also drag in Control layout resolution — a second
/// mechanism that could fail this test for reasons that have nothing to do with
/// button groups.
#[test]
fn a_tscn_button_group_body_reaches_its_member_nodes() {
    let (tree, a, b) = instance(SCENE);

    assert_eq!(
        button_group_of(&tree, a),
        button_group_of(&tree, b),
        "both nodes must land in the same group"
    );
    assert!(
        !button_group_of(&tree, a).is_empty(),
        "and that group must not be the empty (ungrouped) key"
    );
    assert_eq!(
        button_group_buttons(&tree, a),
        vec![a, b],
        "get_buttons() must report both members of the .tscn group"
    );

    for (id, name) in [(a, "A"), (b, "B")] {
        assert!(
            button_group_allows_unpress(&tree, id),
            "`allow_unpress = true` from the [sub_resource] body must reach {name}. \
             False here means the body was dropped at load: the node keeps only a \
             `__resref__button_group` naming `class::id`, so the flag has to be \
             flattened onto every member the way __material__*/__shape__*/\
             __replication__* already are"
        );
    }
}

/// The control for the test above: a group that does not set the flag must not
/// acquire it.
///
/// ⚠️ Without this, a flatten that wrote `true` unconditionally — or a
/// `button_group_allows_unpress` that returned `true` on a missing key — would
/// pass the test above and silently turn every radio group in every project
/// into a set of checkboxes.
#[test]
fn a_tscn_group_without_the_flag_keeps_godots_default() {
    let (tree, a, b) = instance(SCENE_DEFAULT);

    for (id, name) in [(a, "A"), (b, "B")] {
        assert!(
            !button_group_allows_unpress(&tree, id),
            "an unset `allow_unpress` must stay false on {name} — Godot's default"
        );
    }
}
