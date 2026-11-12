//! pat-s3rrv.57 — `ClassName.new()` is answered by ClassDB, not a 26-name list.
//!
//! Godot resolves `SomeClass.new()` through `ClassDB::instantiate`, which works
//! for every non-abstract registered class. Patina answered it from a literal
//! `matches!` of 26 names, so `AnimatedSprite2D.new()`, `Line2D.new()`,
//! `VBoxContainer.new()`, `MeshInstance3D.new()` and hundreds more raised
//! `UndefinedVariable` — **despite being registered in ClassDB**. The class
//! existed and the script simply could not say its name, which reads to a game
//! author like a typo in their own code.
//!
//! # Why this test lives in gdscene
//!
//! The `.new()` node path needs `scene_access` to be `Some` so it can reach
//! `create_node`, and that is installed by the scene-tree host. A
//! gdscript-interop test would get `None` and fail with "requires scene access"
//! before reaching anything this bead changed.
//!
//! ⚠️ `_ready` is dispatched by `LifecycleManager::enter_tree`, **not** by
//! `MainLoop::step` — without the explicit call every var below stays at its
//! declared `null` and the file fails reading `Some(Nil)`, which looks like
//! "`.new()` returned nothing" rather than "the script never ran".

use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager};
use gdvariant::Variant;

/// Constructs five classes that ClassDB knows and the old allow-list did not,
/// spanning 2D, Control and 3D so a fix that only widened one family fails.
///
/// Each is added to the tree and its class name read back, so the assertion is
/// "a live node of the right class arrived", not merely "`.new()` did not raise".
const NEW_GD: &str = "\
extends Node
var anim_cls = \"\"
var line_cls = \"\"
var vbox_cls = \"\"
var mesh_cls = \"\"
var marker_cls = \"\"
var child_count = 0

func _ready():
    var a = AnimatedSprite2D.new()
    add_child(a)
    self.anim_cls = a.get_class()
    var l = Line2D.new()
    add_child(l)
    self.line_cls = l.get_class()
    var v = VBoxContainer.new()
    add_child(v)
    self.vbox_cls = v.get_class()
    var m = MeshInstance3D.new()
    add_child(m)
    self.mesh_cls = m.get_class()
    var k = Marker2D.new()
    add_child(k)
    self.marker_cls = k.get_class()
    self.child_count = get_child_count()
";

/// Godot refuses to construct an abstract class with a specific diagnostic
/// (`gdscript_analyzer.cpp:3622`). Before this bead it fell through to
/// `UndefinedVariable`, telling the author their class name was wrong.
const ABSTRACT_GD: &str = "\
extends Node
var reached_after = false

func _ready():
    var c = CanvasItem.new()
    self.reached_after = true
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn string_var(ml: &MainLoop, node: NodeId, name: &str) -> String {
    match script_var(ml, node, name) {
        Some(Variant::String(s)) => s,
        other => panic!("`{name}` must be a String, got {other:?}"),
    }
}

fn run(src: &str) -> (MainLoop, NodeId) {
    // ClassDB is NOT populated implicitly — without these the whole registry is
    // empty, every ClassDB query answers false, and the generic path can find
    // nothing. Both registrars run because the fixture spans 2D and 3D.
    gdobject::class_db::register_2d_classes();
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

/// **The acceptance test.**
#[test]
fn engine_class_new_routes_through_classdb() {
    let (ml, node) = run(NEW_GD);

    for (var, expected) in [
        ("anim_cls", "AnimatedSprite2D"),
        ("line_cls", "Line2D"),
        ("vbox_cls", "VBoxContainer"),
        ("mesh_cls", "MeshInstance3D"),
        ("marker_cls", "Marker2D"),
    ] {
        assert_eq!(
            string_var(&ml, node, var),
            expected,
            "`{expected}.new()` must produce a live {expected}. An empty string \
             means the script raised before the assignment — the failure the \
             26-name allow-list produced for every class outside it.",
        );
    }

    // The nodes really entered the tree, rather than `.new()` handing back an
    // id that was never parented.
    assert_eq!(
        script_var(&ml, node, "child_count"),
        Some(Variant::Int(5)),
        "all five constructed nodes must be children of the host",
    );
}

/// `ClassDB.instantiate(class)` builds the same live node `.new()` does
/// (pat-s3rrv.57).
///
/// In Godot these are not two mechanisms — `.new()` IS `ClassDB::instantiate` —
/// so the two disagreeing would be its own bug. This asserts they agree on the
/// class that comes back and on the abstract refusal, and that an unregistered
/// name answers `null` rather than raising, which is Godot's contract for a
/// class it cannot build.
const INSTANTIATE_GD: &str = "\
extends Node
var cls = \"\"
var unknown_is_null = false
var child_count = 0

func _ready():
    var s = ClassDB.instantiate(\"Sprite2D\")
    add_child(s)
    self.cls = s.get_class()
    self.unknown_is_null = ClassDB.instantiate(\"NoSuchClassAnywhere\") == null
    self.child_count = get_child_count()
";

#[test]
fn classdb_instantiate_matches_new() {
    let (ml, node) = run(INSTANTIATE_GD);

    assert_eq!(
        string_var(&ml, node, "cls"),
        "Sprite2D",
        "ClassDB.instantiate must build a live Sprite2D, exactly as \
         Sprite2D.new() does — an empty string means it raised",
    );
    assert_eq!(
        script_var(&ml, node, "child_count"),
        Some(Variant::Int(1)),
        "the instantiated node must really parent, not just return an id",
    );
    assert_eq!(
        script_var(&ml, node, "unknown_is_null"),
        Some(Variant::Bool(true)),
        "Godot answers null for a class it cannot build rather than raising; \
         raising here would abort `_ready` and leave this at its initialiser",
    );
}

/// An abstract class is a DIFFERENT failure from an unknown one, and Godot says
/// so. `CanvasItem` is registered (`.abstract_class()`) and derives from `Node`,
/// so a naive `class_exists && is_parent_class(_, "Node")` predicate would
/// happily construct it — this is the assertion that pins the `!is_abstract`
/// half of the rule.
#[test]
fn abstract_class_new_is_refused_with_godot_text() {
    let (ml, node) = run(ABSTRACT_GD);

    // The script raised inside `_ready`, so the statement after the `.new()`
    // never ran and the var keeps its declared value.
    assert_eq!(
        script_var(&ml, node, "reached_after"),
        Some(Variant::Bool(false)),
        "`CanvasItem.new()` must raise; execution continued past it, which \
         means an abstract class was constructed",
    );
}

/// Every non-abstract Node-derived class ClassDB knows is now constructible —
/// the general claim the acceptance's five samples stand for.
///
/// Asserted against ClassDB itself rather than a hand-written list, so the
/// coverage grows automatically as classes are registered. It checks the
/// FACTORY (`class_db::instantiate`), which is the same rule the interpreter's
/// predicate consults, and pins the abstract half from the other direction.
#[test]
fn classdb_instantiate_covers_concrete_nodes_and_refuses_abstract() {
    gdobject::class_db::register_2d_classes();
    gdobject::class_db::register_3d_classes();

    let mut concrete = 0usize;
    for name in gdobject::class_db::get_class_list() {
        if !gdobject::class_db::is_parent_class(&name, "Node") {
            continue;
        }
        if gdobject::class_db::is_abstract(&name) {
            assert!(
                gdobject::class_db::instantiate(&name).is_none(),
                "abstract `{name}` must not instantiate — Godot's ClassDB has no \
                 creation func for it, and returning an object is worse than an \
                 error because the caller gets something that can never behave \
                 like the class it names",
            );
            continue;
        }
        assert!(
            gdobject::class_db::instantiate(&name).is_some(),
            "concrete Node-derived `{name}` must instantiate",
        );
        concrete += 1;
    }

    // Guard against the loop passing vacuously on an empty registry — the
    // failure mode where every ClassDB query answers false and nothing is
    // actually checked.
    assert!(
        concrete > 20,
        "the registry must hold a substantial set of concrete Node classes; \
         only {concrete} found, which means ClassDB was not populated and every \
         assertion above passed vacuously",
    );
}
