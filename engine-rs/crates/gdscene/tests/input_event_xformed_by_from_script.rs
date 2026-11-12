//! pat-s3rrv.14 — `InputEvent.xformed_by()` answers from a script.
//!
//! Godot's `xformed_by(xform, local_ofs := Vector2())` returns a **copy** of the
//! event, offset then transformed. It is what a Control uses to re-express a
//! viewport event in its own local space, and it had no implementation here.
//!
//! # Why this one is implementable and `accumulate` is not
//!
//! `xformed_by` returns a NEW event, so Patina's value-semantics
//! `Variant::Resource` is a perfectly good carrier. `accumulate` by contrast
//! MUTATES the receiver — it only works in Godot because an `InputEvent` is a
//! RefCounted object every holder shares. Through a value Resource an
//! implementation could only ever return `true` while copying nothing, so it is
//! deliberately left unimplemented rather than faked.
//!
//! # The rule these tests exist to pin
//!
//! Transcribed from `upstream/godot/core/input/input_event.cpp`
//! (`InputEventMouseMotion::xformed_by`, line 964), NOT inferred:
//!
//! ```text
//! position   -> xform.xform(position + local_ofs)   affine: translation APPLIES
//! relative   -> xform.basis_xform(relative)         direction: translation does NOT
//! velocity   -> xform.basis_xform(velocity)         direction
//! global_position, screen_relative, screen_velocity -> copied UNCHANGED
//! ```
//!
//! ⚠️ Transforming everything "for consistency" is the obvious mistake and is
//! wrong twice over: it would translate a delta (making `relative` grow by the
//! offset every call) and it would drag the screen-space fields out of screen
//! space. A pure-translation transform separates the two cases exactly.

use gdplatform::input::InputEvent;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

use gdcore::math::Vector2;

/// Re-expresses the motion event through a pure TRANSLATION of (100, 200).
///
/// A translation is the sharpest probe available: it must move `position` and
/// must leave every direction-valued field alone.
const LISTENER: &str = "\
extends Node
var pos_x = -1.0
var pos_y = -1.0
var rel_x = -1.0
var rel_y = -1.0
var gpos_x = -1.0
var srel_x = -1.0

func _input(event):
    if event is InputEventMouseMotion:
        var t = Transform2D(0.0, Vector2(100.0, 200.0))
        var e2 = event.xformed_by(t)
        self.pos_x = e2.position.x
        self.pos_y = e2.position.y
        self.rel_x = e2.relative.x
        self.rel_y = e2.relative.y
        self.gpos_x = e2.global_position.x
        self.srel_x = e2.screen_relative.x
";

fn listener_loop(src: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Listener", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(src, node).expect("script parses")),
    );
    let mut ml = MainLoop::new(tree);
    // One warm-up frame before any event, mirroring the sibling InputEvent
    // method test: the tree's input mirror is refreshed after the script phase.
    ml.step(1.0 / 60.0);
    (ml, node)
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

fn num(ml: &MainLoop, node: NodeId, name: &str) -> f64 {
    match script_var(ml, node, name) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("expected a number for `{name}`, got {other:?}"),
    }
}

fn motion() -> InputEvent {
    InputEvent::MouseMotion {
        position: Vector2::new(5.0, 7.0),
        relative: Vector2::new(3.0, 4.0),
    }
}

/// **The acceptance case**: position moves, and the direction fields do not.
///
/// Both halves in one test on purpose — they are the same mistake seen from two
/// sides, and asserting only the first would pass against an implementation
/// that transformed everything.
#[test]
fn position_is_transformed_and_relative_is_not() {
    let (mut ml, node) = listener_loop(LISTENER);
    ml.push_event(motion());
    ml.step(1.0 / 60.0);

    assert!(
        (num(&ml, node, "pos_x") - 105.0).abs() < 1e-4,
        "position.x must be 5 + 100 = 105 (affine, translation applies); got {}. \
         -1 means `_input` never ran or xformed_by answered nothing",
        num(&ml, node, "pos_x")
    );
    assert!(
        (num(&ml, node, "pos_y") - 207.0).abs() < 1e-4,
        "position.y must be 7 + 200 = 207; got {}",
        num(&ml, node, "pos_y")
    );

    assert!(
        (num(&ml, node, "rel_x") - 3.0).abs() < 1e-4,
        "relative.x must stay 3 — it is a DELTA, and basis_xform excludes the \
         translation. Getting 103 here means the code applied `xform` where it \
         must apply `basis_xform`; got {}",
        num(&ml, node, "rel_x")
    );
    assert!(
        (num(&ml, node, "rel_y") - 4.0).abs() < 1e-4,
        "relative.y must stay 4; got {}",
        num(&ml, node, "rel_y")
    );
}

/// The screen-space fields are copied verbatim — they are deliberately outside
/// the transformed local space (`input_event.cpp` copies them unchanged).
#[test]
fn screen_space_fields_are_left_alone() {
    let (mut ml, node) = listener_loop(LISTENER);
    ml.push_event(motion());
    ml.step(1.0 / 60.0);

    assert!(
        (num(&ml, node, "gpos_x") - 5.0).abs() < 1e-4,
        "global_position.x must stay 5: Godot copies it unchanged rather than \
         transforming it; got {}",
        num(&ml, node, "gpos_x")
    );
    assert!(
        (num(&ml, node, "srel_x") - 3.0).abs() < 1e-4,
        "screen_relative.x must stay 3 for the same reason; got {}",
        num(&ml, node, "srel_x")
    );
}
