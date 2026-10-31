//! pat-mgixk.21 (increment 1) — a script that `extends` another SCRIPT takes
//! its node class from the first NON-script ancestor.
//!
//! Godot ground truth: `extends "res://base.gd"` makes a script inherit the
//! base script's native base class, so a node built from it is that class
//! (`modules/gdscript/gdscript.cpp`, `GDScript::_base`).
//!
//! Patina's `script_base_class` answered `"Node"` for **any** quoted extends.
//! Its only non-test caller is `SceneAccess::instantiate_script_with_values`,
//! which passes the result straight to `Node::new(name, base_class)` — so
//! `load("res://player.gd").new()` built a bare `Node` however specific the
//! base's own `extends` was, and every `Node2D`/`Control` API on the result was
//! absent.
//!
//! Entry point: a GDScript attached to a node and driven through
//! `SceneTree::process_script_ready`, which installs the `SceneTreeAccessor` so
//! `load()` inside `_ready` reaches the same host a live frame would. That is
//! the harness `gdscript_preload_instantiate_spawns_live_scene.rs` established.
//!
//! ⚠️ `gdcore::set_path_roots` is not optional here. Without a project root
//! `res://base.gd` is unreadable, the chain walk finds nothing, and the test
//! would fail for an environmental reason that looks exactly like the bug.
//!
//! ⚠️ This covers the acceptance's "the node's native class is Node2D" claim
//! for the `load(...).new()` path, which is the one `script_base_class`
//! governs. A `.tscn`-attached script takes its class from the scene's `type=`
//! field instead, so that spelling cannot exercise this code at all. The rest
//! of pat-mgixk.21 — inherited members, method-chain lookup and `super` — needs
//! `ClassDef.base` in `interpreter.rs` and is not asserted here.

use std::path::PathBuf;
use std::sync::OnceLock;

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A project root holding `base.gd` (a DIRECT `extends Node2D`) and
/// `player.gd` (`extends "res://base.gd"`), so the two cases differ only by
/// one level of script indirection.
fn res_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!(
            "patina_mgixk21_native_base_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp project dir is creatable");
        std::fs::write(
            dir.join("base.gd"),
            "extends Node2D\n\
             var hp = 10\n\
             func hit():\n\
             \tself.hp = self.hp - 1\n\
             func _ready():\n\
             \tself.hp = self.hp + 5\n",
        )
        .expect("base.gd is writable");
        std::fs::write(dir.join("player.gd"), "extends \"res://base.gd\"\n")
            .expect("player.gd is writable");
        gdcore::set_path_roots(Some(dir.clone()), None);
        dir
    })
}

/// Builds a tree whose `Main` runs `script_src`, fires `_ready`, and hands back
/// the tree plus `Main`'s id.
fn run_ready(script_src: &str) -> (SceneTree, NodeId) {
    res_root();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main_id = tree.add_child(root, Node::new("Main", "Node")).unwrap();
    let script = GDScriptNodeInstance::from_source(script_src, main_id).expect("script parses");
    tree.attach_script(main_id, Box::new(script));
    tree.process_script_ready(main_id);
    (tree, main_id)
}

fn prop(tree: &SceneTree, main_id: NodeId, name: &str) -> Variant {
    tree.get_script(main_id)
        .expect("Main has a script")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

#[test]
fn script_extends_script_resolves_native_class() {
    let (tree, main_id) = run_ready(
        "extends Node\n\
         var direct_class = \"<unset>\"\n\
         var chained_class = \"<unset>\"\n\
         func _ready():\n\
         \tvar d = load(\"res://base.gd\").new()\n\
         \tself.direct_class = d.get_class()\n\
         \tvar n = load(\"res://player.gd\").new()\n\
         \tself.chained_class = n.get_class()\n",
    );

    // CONTROL, asserted first. A direct `extends Node2D` already worked before
    // this change, so if this is not `Node2D` the project root or the fixture
    // is wrong and the real assertion below would be meaningless — a failure
    // here means "harness", not "chain".
    assert_eq!(
        prop(&tree, main_id, "direct_class"),
        Variant::String("Node2D".into()),
        "control: `load()` on a script whose own `extends` is `Node2D` must \
         produce a Node2D. `<unset>` means `_ready` never ran or `load()` \
         returned null; \"Node\" means even the direct case regressed.",
    );

    // 🔑 THE INCREMENT. `player.gd` extends `base.gd`, which extends `Node2D`.
    assert_eq!(
        prop(&tree, main_id, "chained_class"),
        Variant::String("Node2D".into()),
        "a script extending another SCRIPT must take the first non-script \
         ancestor's class. \"Node\" is the pre-fix answer: `script_base_class` \
         returned the default for any quoted `extends` instead of reading the \
         base script and following its `extends`.",
    );
}

/// pat-mgixk.21 (increment 2) — a `var` declared only in the BASE script exists
/// on an instance of the derived script.
///
/// `player.gd` declares nothing at all; `hp = 10` lives only in `base.gd`. So
/// before `instantiate_class` merged the chain, a derived instance started with
/// an empty property set and every inherited method touching `hp` operated on
/// `Nil`.
///
/// ⚠️ Deliberately a DIFFERENT observation from the class test above. That one
/// proves `script_base_class` walked the `extends` text; this one proves the
/// parsed base `ClassDef` was attached and its `instance_vars` merged. Either
/// could pass while the other failed.
#[test]
fn an_inherited_var_exists_on_the_derived_instance() {
    let (tree, main_id) = run_ready(
        "extends Node\n\
         var inherited_hp = -1\n\
         func _ready():\n\
         \tvar n = load(\"res://player.gd\").new()\n\
         \tself.inherited_hp = n.hp\n",
    );

    assert_eq!(
        prop(&tree, main_id, "inherited_hp"),
        Variant::Int(10),
        "`hp = 10` is declared ONLY in base.gd, so reading 10 off an instance \
         of player.gd proves the base script was parsed and its instance_vars \
         merged into the derived instance. `-1` means `_ready` never \
         assigned; Nil means the property did not exist on the instance.",
    );
}

/// pat-mgixk.21 (increment 3) — a method declared only in the BASE script is
/// callable on an instance of the derived script.
///
/// `player.gd` declares no methods at all; `hit()` lives only in `base.gd`.
///
/// ⚠️ This exercises BOTH halves of the method fix at once, which is why it is
/// worth having beside the var test:
///   * `has_method` must answer `true` for an inherited name, because
///     `SceneTree::call_script_method` SKIPS anything it reports as undefined —
///     so a wrong answer there is a silent no-op, not an error; and
///   * `call_instance_method_impl` must find the `FuncDef` by walking the
///     chain, instead of raising `UndefinedFunction`.
/// A failure cannot tell you which half broke, but either half alone leaves
/// `hp` at 10.
#[test]
fn an_inherited_method_is_callable_on_the_derived_instance() {
    let (tree, main_id) = run_ready(
        "extends Node\n\
         var hp_after = -1\n\
         func _ready():\n\
         \tvar n = load(\"res://player.gd\").new()\n\
         \tn.hit()\n\
         \tself.hp_after = n.hp\n",
    );

    assert_eq!(
        prop(&tree, main_id, "hp_after"),
        Variant::Int(9),
        "`hit()` is declared ONLY in base.gd and decrements the inherited \
         `hp`, so 9 proves the call reached the base implementation. 10 means \
         the call was skipped (has_method said the method does not exist) or \
         it ran against an unmerged `hp`; -1 means `_ready` aborted, most \
         likely on UndefinedFunction from the method lookup.",
    );
}

/// pat-mgixk.21 (increment 4) — `super()` runs the parent's version of the
/// method it is written in, and `super.method()` names one explicitly.
///
/// The arithmetic is the discriminator, which is why `_ready` adds 5 and
/// `hit()` subtracts 1 rather than both touching the same amount:
///
/// | behaviour                                   | `hp` |
/// |---------------------------------------------|------|
/// | correct: base `_ready` (+5) then `hit` (-1) |  14  |
/// | `super()` wrongly ran `_init` (the old bug) |   9  |
/// | `super.hit()` dispatched on Nil (old bug)   |  15  |
/// | neither worked                              |  10  |
///
/// So a single number says which half failed, instead of just "not 14".
///
/// ⚠️ The script is attached by SOURCE with `extends "res://base.gd"`, so
/// `run_class` has to load the base itself — this also covers the case where
/// the deriving script never existed as a file.
#[test]
fn super_calls_reach_the_parent_implementation() {
    let (tree, main_id) = run_ready(
        "extends \"res://base.gd\"\n\
         var child_ran = false\n\
         func _ready():\n\
         \tsuper()\n\
         \tself.child_ran = true\n\
         \tsuper.hit()\n",
    );

    // The child's own body must still run after `super()` returns.
    assert_eq!(
        prop(&tree, main_id, "child_ran"),
        Variant::Bool(true),
        "`super()` must return control to the child's body, not replace it",
    );

    assert_eq!(
        prop(&tree, main_id, "hp"),
        Variant::Int(14),
        "expected 10 + 5 (the base `_ready`, reached by `super()`) - 1 (the \
         base `hit`, reached by `super.hit()`). 9 means `super()` ran the \
         parent's `_init` instead of its `_ready` — the pre-fix behaviour. 15 \
         means `super.hit()` did nothing because `super` evaluated to Nil. 10 \
         means neither reached the parent at all.",
    );
}
