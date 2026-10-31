//! pat-mgixk.22 acceptance: the ClassDB accessor methods and `hide()`/`show()`
//! reach a node's real properties, and a hidden Control leaves the hit-test and
//! the draw list.
//!
//! ⚠️ **Read this before treating a pass here as proof of new work.** The
//! implementation this test covers was already in the tree when the bead was
//! assigned — `pat-mgixk.8` (closed) landed the generic accessor dispatch
//! (`core_node_methods::call_accessor_backed_method`, reached from the last arm
//! of `scripting::call_node_method`), and the `hide`/`show`/`set_visible`/
//! `is_visible` arms sit a few hundred lines above it. What was missing was any
//! test at all: per pat-mgixk.22's own verifier note,
//! `grep -rl '\.hide()' -e '\.show()'` over `engine-rs` found **nothing**, so
//! the most common GDScript calls in existence had zero coverage.
//!
//! So this file is a regression guard, not a demonstration of a fix. It will
//! pass with the implementation reverted, because the implementation is not
//! mine to revert.
//!
//! What it pins, each of which is a distinct way the feature can rot:
//!
//! * `set_text("hi")` must land in the node's **`text` property**, not in a
//!   script-local shadow — asserted by reading the node's property store
//!   directly, not just the script's own var.
//! * `get_text()` must read that property back. A getter that returned the
//!   ClassDB default would still answer `""` here and look plausible.
//! * `hide()` must write `visible = false`, and the hidden Panel must then be
//!   **skipped by `control_at_point`** and **absent from the draw items**.
//!   Those two are the ones a property-only implementation gets wrong: writing
//!   the key without the render path reading it is exactly the "wired but
//!   dead" shape the audit is about.
//!
//! The acceptance text names `build_control_render_items`; no such function
//! exists. The real draw-list builder is
//! `control_render::control_draw_items`, which is what every other control
//! test uses, so that is what this drives.

use gdcore::math::Vector2;
use gdscene::control::control_at_point;
use gdscene::control_render::{control_draw_items, ControlDrawItem};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

const VIEWPORT: Vector2 = Vector2::new(400.0, 300.0);

/// The hidden Panel's authored rect, as `offset_*` places it.
const PANEL_RECT: (f32, f32, f32, f32) = (200.0, 200.0, 300.0, 260.0);

/// Whether any `Rect` item is painted inside the Panel's authored rect.
fn panel_is_painted(items: &[ControlDrawItem]) -> bool {
    let (x0, y0, x1, y1) = PANEL_RECT;
    items.iter().any(|item| match item {
        ControlDrawItem::Rect { rect, .. } => {
            rect.position.x >= x0 - 1.0
                && rect.position.y >= y0 - 1.0
                && rect.position.x <= x1 + 1.0
                && rect.position.y <= y1 + 1.0
        }
        _ => false,
    })
}

/// A Label and a Panel at known, non-overlapping rects so the hit-test and the
/// draw list can each be checked for one specific node.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Control"]
offset_right = 400.0
offset_bottom = 300.0

[node name="Lbl" type="Label" parent="."]
offset_left = 0.0
offset_top = 0.0
offset_right = 120.0
offset_bottom = 30.0

[node name="Pnl" type="Panel" parent="."]
offset_left = 200.0
offset_top = 200.0
offset_right = 300.0
offset_bottom = 260.0
"#;

/// Assignments go through `self.<var>`, the spelling the other script-dispatch
/// tests use and that `list_properties` reads back. `stage` names the call that
/// raised if `_ready` aborts part-way.
const SCRIPT: &str = r#"
extends Control

var got_text
var panel_visible
var stage = 0
var ran = false

func _ready():
    $Lbl.set_text("hi")
    self.stage = 1
    $Pnl.hide()
    self.stage = 2
    self.got_text = $Lbl.get_text()
    self.stage = 3
    self.panel_visible = $Pnl.is_visible()
    self.stage = 4
    self.ran = true
"#;

/// Which call each `stage` value means, so a TypeError names the method that
/// has no dispatch arm instead of just reporting "`_ready` did not finish".
const STAGES: [&str; 5] = [
    "$Lbl.set_text(\"hi\")",
    "$Pnl.hide()",
    "$Lbl.get_text()",
    "$Pnl.is_visible()",
    "(all calls completed)",
];

fn script_var(tree: &SceneTree, node: gdscene::node::NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

fn find(tree: &SceneTree, name: &str) -> gdscene::node::NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == name)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("no node named {name}"))
}

/// ⚠️ The inverse of the usual registered-but-undispatched bug, and it reads
/// the opposite way round.
///
/// `$Panel.hide()` has always worked — `scripting::call_node_method` has the
/// arm. But CanvasItem registered **no** `show`/`hide`/`is_visible_in_tree` in
/// ClassDB (Godot binds all three, `canvas_item.cpp:1357-1359`), and
/// `ClassDB::has_method` answers only from the `methods` lists along the
/// inheritance chain. So `has_method("hide")` returned **false** on every
/// Control, and a script written the defensive way —
/// `if node.has_method("hide"): node.hide()` — skipped a call that would have
/// worked.
///
/// The `hide`/`show` registrations that do exist belong to **Node3D**
/// (class_db.rs:1553-1574) and never reach a Control: Panel and Label descend
/// from CanvasItem.
#[test]
fn canvas_item_registers_the_visibility_methods_it_dispatches() {
    // The registry starts EMPTY and only this call populates it.
    gdobject::class_db::register_all_classes();

    for class in ["CanvasItem", "Control", "Panel", "Label"] {
        for method in ["show", "hide", "is_visible_in_tree"] {
            assert!(
                gdobject::class_db::class_has_method(class, method),
                "{class}.{method}() is dispatched but was not registered, so \
                 has_method() denied a call that works"
            );
        }
    }

    // A control still must not claim a method it genuinely has no arm for —
    // otherwise this registration would just move the lie.
    assert!(
        !gdobject::class_db::class_has_method("Panel", "no_such_method_xyz"),
        "registration must not make has_method answer true for everything"
    );
}

#[test]
fn control_accessor_methods_reach_properties() {
    // ⚠️ REQUIRED, and its absence is what made the first version of this test
    // fail. The accessor dispatch resolves a method by looking it up among the
    // class's registered accessors — `resolve_accessor` calls
    // `class_db::get_property_list(class_name)` (core_node_methods.rs:331) —
    // and `class_db`'s registry starts **EMPTY**. With nothing registered,
    // `Label` has no `text` property, no accessor matches `set_text`, the
    // dispatch declines, and the call raises. The implementation was fine; the
    // test simply had not populated the registry it reads.
    //
    // ⚠️ And it is needed HERE specifically, not just somewhere in the file:
    // nextest runs each `#[test]` in its own process, so the
    // `register_all_classes()` in the sibling test below does not carry over.
    gdobject::class_db::register_all_classes();

    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let script = GDScriptNodeInstance::from_source(SCRIPT, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);

    // ⚠️ Baseline BEFORE `_ready` runs, while the Panel is still visible.
    // Without it the "hidden Panel is absent from the draw list" assertion
    // below is vacuous: a Panel that is never painted at all would satisfy it
    // just as well as one correctly skipped for being hidden.
    let before = control_draw_items(ml.tree_mut(), scene_root, &Theme::default(), VIEWPORT);
    assert!(
        panel_is_painted(&before),
        "the Panel must be painted while visible, or the absence check after \
         hide() proves nothing. Items: {before:?}"
    );

    ml.tree_mut().process_script_ready(scene_root);

    // First: an undispatched method raises and aborts `_ready`, which from here
    // is indistinguishable from "the var was never declared".
    let stage = match script_var(ml.tree(), scene_root, "stage") {
        Variant::Int(i) => i as usize,
        other => panic!("`stage` should be an Int, got {other:?}"),
    };
    assert_eq!(
        script_var(ml.tree(), scene_root, "ran"),
        Variant::Bool(true),
        "`_ready` aborted after stage {stage} ({}), so the call that raised is \
         `{}` — that method has no dispatch arm",
        STAGES
            .get(stage.saturating_sub(1))
            .copied()
            .unwrap_or("(none yet)"),
        STAGES.get(stage).copied().unwrap_or("(unknown)"),
    );

    let label = find(ml.tree(), "Lbl");
    let panel = find(ml.tree(), "Pnl");

    // -- the setter reached the PROPERTY, not a script-local shadow -------
    assert_eq!(
        ml.tree().get_node(label).unwrap().get_property("text"),
        Variant::String("hi".into()),
        "set_text must write the node's `text` property — a script-local write \
         would leave this empty while the script's own var looked right"
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "got_text"),
        Variant::String("hi".into()),
        "get_text must read the property back; returning the ClassDB default \
         would answer \"\" here and still look plausible"
    );

    // -- hide() wrote `visible`, and is_visible() read it ----------------
    assert_eq!(
        ml.tree().get_node(panel).unwrap().get_property("visible"),
        Variant::Bool(false),
        "hide() must write visible = false"
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "panel_visible"),
        Variant::Bool(false),
        "is_visible() must report the property hide() just wrote"
    );

    // -- ...and the render path actually honours it -----------------------
    //
    // This is the half a property-only implementation gets wrong: writing the
    // key without the hit-test and draw list reading it is "wired but dead".
    let hit = control_at_point(ml.tree(), Vector2::new(250.0, 230.0));
    assert_ne!(
        hit,
        Some(panel),
        "a hidden Panel must not be hit-tested; control_at_point returned it"
    );

    let items = control_draw_items(ml.tree_mut(), scene_root, &Theme::default(), VIEWPORT);
    assert!(
        items.iter().any(|item| matches!(
            item,
            ControlDrawItem::Text { text, .. } if text == "hi"
        )),
        "the Label's new text must reach the draw list — set_text is only \
         observable to a player through this. Items: {items:?}"
    );
    // ...and it is gone now that it is hidden. Paired with the `before`
    // assertion above, this is a real before/after rather than an absence that
    // was always true.
    assert!(
        !panel_is_painted(&items),
        "a hidden Panel must leave the draw list — it was painted before \
         hide() and must not be painted after. Items: {items:?}"
    );
}
