//! pat-s3rrv.57 — the ClassDB singleton's reflection methods reach GDScript.
//!
//! `ClassDB` dispatched 12 of Godot's 30 methods; everything else hit the
//! catch-all and raised `UndefinedFunction`. The frustrating part is that the
//! Rust side of several already existed — `is_parent_class`, `get_property_list`,
//! `get_method_list`, `get_integer_constant_list` are all in
//! `gdobject::class_db` — with no script caller. The data was present and simply
//! unspeakable from GDScript, which is the same shape as the `.new()` allow-list
//! this bead's acceptance covers.
//!
//! Every assertion below is a call a real tool makes: a save system walking
//! `class_get_property_list`, an inspector asking for a default to show a revert
//! arrow, a plugin enumerating subclasses.

use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager};
use gdvariant::Variant;

/// ⚠️ `_ready` is dispatched by `LifecycleManager::enter_tree`, not by
/// `MainLoop::step`. Without the explicit call every var stays at its declared
/// value and the failures read as "ClassDB returned nothing".
const REFLECT_GD: &str = "\
extends Node
var sprite_is_node = false
var node_is_sprite = true
var enabled = false
var api_type = -1
var has_const = false
var inheritors = 0
var prop_count = 0
var method_count = 0
var arg_count = -1
var default_visible = null
var own_consts = -1
var inherited_consts = -1
var own_enums = -1
var inherited_enums = -1
var clip_enum = \"unset\"
var plain_const_enum = \"unset\"
var flags_is_bitfield = false
var plain_is_bitfield = true
var sig_has_name = false
var missing_sig_size = -1

func _ready():
    self.sprite_is_node = ClassDB.is_parent_class(\"Sprite2D\", \"Node\")
    self.node_is_sprite = ClassDB.is_parent_class(\"Node\", \"Sprite2D\")
    self.enabled = ClassDB.is_class_enabled(\"Sprite2D\")
    self.api_type = ClassDB.class_get_api_type(\"Sprite2D\")
    self.has_const = ClassDB.class_has_integer_constant(\"Node\", \"NOTIFICATION_READY\")
    self.inheritors = ClassDB.get_inheriters_from_class(\"CanvasItem\").size()
    self.prop_count = ClassDB.class_get_property_list(\"CanvasItem\").size()
    self.method_count = ClassDB.class_get_method_list(\"Node\").size()
    self.arg_count = ClassDB.class_get_method_argument_count(\"Node\", \"add_child\")
    self.default_visible = ClassDB.class_get_property_default_value(\"CanvasItem\", \"visible\")
    self.own_consts = ClassDB.class_get_integer_constant_list(\"Node2D\", true).size()
    self.inherited_consts = ClassDB.class_get_integer_constant_list(\"Node2D\", false).size()
    self.own_enums = ClassDB.class_get_enum_list(\"Node2D\", true).size()
    self.inherited_enums = ClassDB.class_get_enum_list(\"AudioEffectDistortion\", false).size()
    self.clip_enum = ClassDB.class_get_integer_constant_enum(\"AudioEffectDistortion\", \"MODE_CLIP\")
    self.plain_const_enum = ClassDB.class_get_integer_constant_enum(\"Node\", \"NOTIFICATION_READY\")
    self.flags_is_bitfield = ClassDB.is_class_enum_bitfield(\"SpriteBase3D\", \"DrawFlags\")
    self.plain_is_bitfield = ClassDB.is_class_enum_bitfield(\"AudioEffectDistortion\", \"Mode\")
    self.sig_has_name = ClassDB.class_get_signal(\"Node\", \"ready\").has(\"name\")
    self.missing_sig_size = ClassDB.class_get_signal(\"Node\", \"definitely_not_a_signal\").size()
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn int_var(ml: &MainLoop, node: NodeId, name: &str) -> i64 {
    match script_var(ml, node, name) {
        Some(Variant::Int(v)) => v,
        other => panic!(
            "`{name}` must be an Int, got {other:?} — a Nil here means the \
                         ClassDB arm raised and the assignment never happened"
        ),
    }
}

fn bool_var(ml: &MainLoop, node: NodeId, name: &str) -> bool {
    match script_var(ml, node, name) {
        Some(Variant::Bool(v)) => v,
        other => panic!("`{name}` must be a Bool, got {other:?}"),
    }
}

fn run(src: &str) -> (MainLoop, NodeId) {
    gdobject::class_db::register_2d_classes();
    // SpriteBase3D owns the DrawFlags bitfield asserted below.
    gdobject::class_db::register_3d_classes();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let node = tree.add_child(root_id, Node::new("Host", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(src, node).unwrap()),
    );
    LifecycleManager::enter_tree(&mut tree, node);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    (ml, node)
}

#[test]
fn classdb_singleton_reflection_from_script() {
    let (ml, node) = run(REFLECT_GD);

    // -- inheritance, both directions ---------------------------------------
    //
    // The negative matters as much as the positive: a stub returning `true`
    // unconditionally would satisfy the first assertion alone.
    assert!(
        bool_var(&ml, node, "sprite_is_node"),
        "Sprite2D derives from Node",
    );
    assert!(
        !bool_var(&ml, node, "node_is_sprite"),
        "Node does NOT derive from Sprite2D — is_parent_class must be directional",
    );

    assert!(
        bool_var(&ml, node, "enabled"),
        "a registered class is an enabled one",
    );
    assert_eq!(
        int_var(&ml, node, "api_type"),
        0,
        "every class Patina registers is a built-in, so API_CORE",
    );
    assert!(
        bool_var(&ml, node, "has_const"),
        "Node must report NOTIFICATION_READY as an integer constant it owns",
    );

    // -- the list-returning methods ------------------------------------------
    //
    // Bounds rather than exact counts: these grow as classes gain metadata, and
    // an exact number would turn every registration into a test failure. What
    // must hold is that they are non-empty, which is what distinguishes a wired
    // arm from one returning a default-constructed Array.
    assert!(
        int_var(&ml, node, "inheritors") > 0,
        "CanvasItem has descendants (Node2D, Control, Sprite2D...); zero means \
         get_inheriters_from_class walked nothing",
    );
    assert!(
        int_var(&ml, node, "prop_count") > 0,
        "CanvasItem registers properties (visible, modulate...); zero means the \
         property list came back empty",
    );
    assert!(
        int_var(&ml, node, "method_count") > 0,
        "Node registers methods; zero means the method list came back empty",
    );
    assert_eq!(
        int_var(&ml, node, "arg_count"),
        1,
        "Node.add_child takes one required argument",
    );

    // -- `no_inheritance` is honoured, not silently dropped ------------------
    //
    // This was a WRONG ANSWER rather than a missing feature: the arms took the
    // flag and ignored it, so "what does Node2D declare itself" returned
    // everything Node and Object declare too. Node2D registers no integer
    // constants of its own while Node registers several, which makes the two
    // calls differ by construction.
    assert_eq!(
        int_var(&ml, node, "own_consts"),
        0,
        "Node2D declares no integer constants of its own; a non-zero count means \
         `no_inheritance = true` was dropped and the ancestors leaked in",
    );
    assert!(
        int_var(&ml, node, "inherited_consts") > 0,
        "with inheritance Node2D must see Node's NOTIFICATION_* constants — zero \
         here would mean the walk is broken in the other direction",
    );
    assert_eq!(
        int_var(&ml, node, "own_enums"),
        0,
        "Node2D declares no enums of its own",
    );
    assert!(
        int_var(&ml, node, "inherited_enums") > 0,
        "AudioEffectDistortion declares a `Mode` enum, so the inheriting form \
         must report at least one",
    );

    // -- which enum owns a constant ------------------------------------------
    assert_eq!(
        script_var(&ml, node, "clip_enum"),
        Some(Variant::String("Mode".to_string())),
        "MODE_CLIP belongs to AudioEffectDistortion's `Mode` enum",
    );
    // Godot answers the empty StringName for a constant that is not part of an
    // enum — a plain constant is a normal answer, not a lookup failure.
    assert_eq!(
        script_var(&ml, node, "plain_const_enum"),
        Some(Variant::String(String::new())),
        "NOTIFICATION_READY is a bare constant, so its owning enum is empty",
    );

    // -- flags enums are distinguishable from plain ones ---------------------
    //
    // ⚠️ Which enums are bitfields comes from Godot's doc XML (`is_bitfield`),
    // NOT from the constant values. `Node.DuplicateFlags` has values 1/2/4/8 and
    // is NOT marked a bitfield upstream — inferring from powers of two would
    // have flagged it wrongly. Cross-checking the registry against the oracle
    // found exactly four registered bitfield enums; DrawFlags is one.
    assert!(
        bool_var(&ml, node, "flags_is_bitfield"),
        "SpriteBase3D.DrawFlags is marked is_bitfield upstream",
    );
    assert!(
        !bool_var(&ml, node, "plain_is_bitfield"),
        "AudioEffectDistortion.Mode is a plain enumeration — a stub answering \
         true for everything would pass the positive case alone",
    );

    // -- a single signal's descriptor ----------------------------------------
    assert!(
        bool_var(&ml, node, "sig_has_name"),
        "class_get_signal must describe Node's `ready` signal",
    );
    assert_eq!(
        int_var(&ml, node, "missing_sig_size"),
        0,
        "an unknown signal yields an EMPTY dictionary, not an error — Godot \
         treats a miss as a normal answer",
    );

    // -- a default value round-trips as its real type ------------------------
    //
    // `visible` is a Bool defaulting to true. Getting `Nil` would mean the
    // property was not found; getting an Int would mean the Variant was
    // flattened on the way out.
    assert_eq!(
        script_var(&ml, node, "default_visible"),
        Some(Variant::Bool(true)),
        "CanvasItem.visible defaults to true and must arrive as a Bool",
    );
}
