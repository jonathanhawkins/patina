//! pat-mgixk.108 (part 2) — typed declarations convert, and a `.tscn` can carry
//! a typed `Dictionary` literal.
//!
//! # What this test is load-bearing on
//!
//! 🔑 Be precise about this, because most of the acceptance criteria for part 2
//! **already passed before this bead**. `coerce_for_declared_slot`
//! (`container_typing.rs`) was already wired into all four paths the bead lists
//! as missing: local `var` declarations, the assignment writer, class-member
//! init / `@onready` in `instantiate_class`, and parameter binding in
//! `coerce_param_value`. So `var speed: float = 100` already stored `100.0`.
//! Those clauses are asserted here as **regression cover**, not as new work.
//!
//! The one genuinely missing piece was the loader: `Dictionary[K, V]({…})` —
//! the shape Godot 4.4+ writes a typed dictionary in
//! (`core/variant/variant_parser.cpp:2241`) — had no branch in
//! `gdresource::loader::parse_variant_value`, so it fell through every case to
//! `unrecognized value` and **the whole scene failed to load**. That is what
//! `scene_carries_a_typed_dictionary_literal` covers, and it is why
//! `PackedScene::from_tscn` is the entry point rather than a hand-built tree:
//! `packed_scene.rs` parses every node property through exactly that function.
//!
//! # Entry points
//!
//! The scene clause enters through `PackedScene::from_tscn` +
//! `add_packed_scene_to_tree`. The script clauses attach a `GDScript` to the
//! instanced scene's own node and run it through `MainLoop`.
//!
//! ⚠️ The script is attached in Rust rather than resolved from a
//! `script = ExtResource(..)` line, because `add_packed_scene_to_tree` does not
//! attach scripts — that happens in the boot path — and no existing test
//! harness dispatches `_ready` for a `.tscn`-declared script. The scene is
//! still what is loaded and instanced; only the script binding is explicit.

use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::{Variant, VariantType};

/// A scene carrying a typed dictionary exactly as Godot 4.4+ writes one.
const SCENE: &str = r#"[gd_scene format=3]

[node name="World" type="Node"]
d = Dictionary[String, int]({"a": 1})
"#;

/// `completed` is the LAST statement: a conversion failure that aborts `_ready`
/// would otherwise leave every property at its declared initialiser, which
/// reads the same as "the engine computed the wrong value".
const SCRIPT: &str = "\
extends Node
var speed: float = 100
var items: Array[int] = []
var ratio = 0
var typed = false
var r = 0
var completed = false
func f(a: float):
\treturn a / 3
func _ready():
\tself.ratio = self.speed / 3
\tself.typed = self.items.is_typed()
\tself.r = f(1)
\tself.completed = true
";

/// Appending a String to an `Array[int]` must raise, so the statement after it
/// never runs. Its own node, so the abort cannot truncate the script above.
const APPEND_SCRIPT: &str = "\
extends Node
var items: Array[int] = []
var reached = false
var survived = false
func _ready():
\tself.reached = true
\tself.items.append(\"x\")
\tself.survived = true
";

fn script_var(ml: &MainLoop, node: NodeId, prop: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("node has a script")
        // `get_property`, not `list_properties().default_value` — the latter is
        // the parse-time initialiser and would report the fixture back to us.
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

fn float_of(v: Variant, what: &str) -> f64 {
    match v {
        Variant::Float(f) => f,
        other => panic!(
            "{what} should be a Float, got {other:?}. An Int here means the \
             declared `float` type never converted the value, so the division \
             that produced it was integer division"
        ),
    }
}

// ---------------------------------------------------------------------------

/// **The acceptance test.** A `.tscn` property holding `Dictionary[String, int]`
/// loads as a genuinely typed dictionary.
#[test]
fn typed_declaration_conversion_test() {
    let scene = PackedScene::from_tscn(SCENE).expect(
        "the scene must PARSE. Before this bead `parse_variant_value` had no \
         `Dictionary[` branch, so this literal fell through to `unrecognized \
         value` and the whole scene failed to load",
    );
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    // -- (a) the typed dictionary survived the load, with BOTH types ----------
    match tree
        .get_node(world)
        .expect("scene root exists")
        .get_property("d")
    {
        Variant::Dictionary(d) => {
            assert_eq!(
                (d.typed_key_builtin(), d.typed_value_builtin()),
                (Some(VariantType::String), Some(VariantType::Int)),
                "`Dictionary[String, int]({{...}})` must load CARRYING its key \
                 and value types. `(None, None)` means the entries were parsed \
                 but the types were dropped, which is the silent half of the \
                 bug: the dictionary looks right and `is_typed()` answers false"
            );
            assert_eq!(
                d.get(Variant::String("a".to_string())),
                Some(Variant::Int(1)),
                "...and the entries must still be there"
            );
        }
        other => panic!("`d` should be a Dictionary, got {other:?}"),
    }

    // -- (b) declared conversions, as regression cover ------------------------
    let script_node = tree.add_child(world, Node::new("Typed", "Node")).unwrap();
    let append_node = tree
        .add_child(world, Node::new("Appender", "Node"))
        .unwrap();
    tree.attach_script(
        script_node,
        Box::new(GDScriptNodeInstance::from_source(SCRIPT, script_node).expect("script parses")),
    );
    tree.attach_script(
        append_node,
        Box::new(
            GDScriptNodeInstance::from_source(APPEND_SCRIPT, append_node)
                .expect("append script parses"),
        ),
    );

    let mut ml = MainLoop::new(tree);
    ml.enter_tree_with_scripts(world);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, script_node, "completed"),
        Variant::Bool(true),
        "`_ready` must run to completion; false means a conversion raised and \
         aborted it, making every value below meaningless"
    );
    let ratio = float_of(script_var(&ml, script_node, "ratio"), "ratio");
    assert!(
        (ratio - 100.0 / 3.0).abs() < 1e-9,
        "`var speed: float = 100` must store 100.0, so `speed / 3` is 33.333…, \
         not 33. Got {ratio}"
    );
    let r = float_of(script_var(&ml, script_node, "r"), "r");
    assert!(
        (r - 1.0 / 3.0).abs() < 1e-9,
        "a parameter declared `a: float` must convert its Int argument, so \
         `f(1)` returns 0.333…, not 0. Got {r}"
    );
    assert_eq!(
        script_var(&ml, script_node, "typed"),
        Variant::Bool(true),
        "`var items: Array[int] = []` must build a genuinely typed array, so \
         `is_typed()` answers true"
    );

    // -- (c) ...and the typing is enforced, not cosmetic ----------------------
    // Both flags matter: `survived == false` alone would also hold if `_ready`
    // had never run on this node.
    assert_eq!(
        script_var(&ml, append_node, "reached"),
        Variant::Bool(true),
        "precondition: `Appender._ready` must have run, or the next assertion \
         is vacuous"
    );
    assert_eq!(
        script_var(&ml, append_node, "survived"),
        Variant::Bool(false),
        "appending a String to an `Array[int]` must RAISE and abort the \
         function. True means the element type is decorative"
    );
}
