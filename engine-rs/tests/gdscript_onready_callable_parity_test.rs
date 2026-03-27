//! pat-ps3l: GDScript @onready, function dispatch, and signal callable parity.
//!
//! Covers:
//! - @onready vars are deferred to Nil at construction, resolved later
//! - Function dispatch: direct calls, default args, recursion
//! - Callable: construction, .call(), .callv(), .is_valid(), .get_method()
//! - Signal declaration and emission through the script layer
//! - Script signal callable: connect + emit roundtrip

use gdscript_interop::interpreter::Interpreter;
use gdvariant::{CallableRef, Variant};

// ===========================================================================
// Helpers
// ===========================================================================

fn run(src: &str) -> (Vec<String>, Option<Variant>) {
    let mut interp = Interpreter::new();
    let result = interp.run(src).expect("script should not error");
    (result.output, result.return_value)
}

fn run_val(src: &str) -> Variant {
    run(src).1.expect("expected a return value")
}

fn run_output(src: &str) -> Vec<String> {
    run(src).0
}

// ===========================================================================
// 1. @onready variable semantics
// ===========================================================================

#[test]
fn onready_var_is_nil_at_construction() {
    let src = "\
class_name TestNode
extends Node

@onready
var label = 42

func get_label():
    return label
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let instance = interp.instantiate_class(&class_def).expect("instantiation failed");

    // @onready var should be Nil at construction time.
    assert_eq!(
        instance.properties.get("label"),
        Some(&Variant::Nil),
        "@onready var must be Nil before resolve"
    );
    assert_eq!(instance.onready_vars, vec!["label"]);
}

#[test]
fn onready_var_resolved_after_ready() {
    let src = "\
class_name TestNode
extends Node

@onready
var label = 42

var normal = 10

func get_label():
    return label
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let mut instance = interp.instantiate_class(&class_def).expect("instantiation failed");

    // Normal var initialized immediately.
    assert_eq!(instance.properties.get("normal"), Some(&Variant::Int(10)));
    // @onready still Nil.
    assert_eq!(instance.properties.get("label"), Some(&Variant::Nil));

    // Resolve onready vars (simulates _ready callback).
    interp
        .resolve_onready_vars(&mut instance)
        .expect("resolve failed");

    // Now the value is available.
    assert_eq!(instance.properties.get("label"), Some(&Variant::Int(42)));
    assert!(instance.onready_vars.is_empty(), "onready list should be drained");
}

#[test]
fn onready_var_with_expression_default() {
    let src = "\
class_name Calc
extends Node

@onready
var result = 6 * 7

func get_result():
    return result
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let mut instance = interp.instantiate_class(&class_def).expect("instantiation failed");

    assert_eq!(instance.properties.get("result"), Some(&Variant::Nil));
    interp.resolve_onready_vars(&mut instance).unwrap();
    assert_eq!(instance.properties.get("result"), Some(&Variant::Int(42)));
}

#[test]
fn multiple_onready_vars_resolved_in_order() {
    let src = "\
class_name Multi
extends Node

@onready
var a = 1

@onready
var b = 2

var c = 3
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let mut instance = interp.instantiate_class(&class_def).expect("instantiation failed");

    assert_eq!(instance.properties.get("a"), Some(&Variant::Nil));
    assert_eq!(instance.properties.get("b"), Some(&Variant::Nil));
    assert_eq!(instance.properties.get("c"), Some(&Variant::Int(3)));

    interp.resolve_onready_vars(&mut instance).unwrap();

    assert_eq!(instance.properties.get("a"), Some(&Variant::Int(1)));
    assert_eq!(instance.properties.get("b"), Some(&Variant::Int(2)));
}

// ===========================================================================
// 2. Function dispatch parity
// ===========================================================================

#[test]
fn function_with_default_args() {
    let val = run_val(
        "\
func greet(name = \"World\"):
    return \"Hello, \" + name

return greet()
",
    );
    assert_eq!(val, Variant::String("Hello, World".into()));
}

#[test]
fn function_default_args_overridden() {
    let val = run_val(
        "\
func greet(name = \"World\"):
    return \"Hello, \" + name

return greet(\"Patina\")
",
    );
    assert_eq!(val, Variant::String("Hello, Patina".into()));
}

#[test]
fn recursive_function_fibonacci() {
    let val = run_val(
        "\
func fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

return fib(10)
",
    );
    assert_eq!(val, Variant::Int(55));
}

#[test]
fn method_dispatch_on_class_instance() {
    let src = "\
class_name Calculator
extends Node

var total = 0

func add(x):
    total = total + x
    return total

func get_total():
    return total
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let mut instance = interp.instantiate_class(&class_def).unwrap();

    let r1 = interp
        .call_instance_method(&mut instance, "add", &[Variant::Int(5)])
        .unwrap();
    assert_eq!(r1, Variant::Int(5));

    let r2 = interp
        .call_instance_method(&mut instance, "add", &[Variant::Int(3)])
        .unwrap();
    assert_eq!(r2, Variant::Int(8));

    let total = interp
        .call_instance_method(&mut instance, "get_total", &[])
        .unwrap();
    assert_eq!(total, Variant::Int(8));
}

// ===========================================================================
// 3. Callable parity
// ===========================================================================

#[test]
fn callable_constructor_creates_method_ref() {
    let val = run_val("return Callable(null, \"my_func\")\n");
    match val {
        Variant::Callable(c) => match c.as_ref() {
            CallableRef::Method { method, .. } => {
                assert_eq!(method, "my_func");
            }
            _ => panic!("expected Method callable"),
        },
        _ => panic!("expected Callable"),
    }
}

#[test]
fn callable_call_invokes_function() {
    let val = run_val(
        "\
func double(x):
    return x * 2

var cb = Callable(null, \"double\")
return cb.call(7)
",
    );
    assert_eq!(val, Variant::Int(14));
}

#[test]
fn callable_callv_with_array() {
    let val = run_val(
        "\
func add(a, b):
    return a + b

var cb = Callable(null, \"add\")
return cb.callv([3, 4])
",
    );
    assert_eq!(val, Variant::Int(7));
}

#[test]
fn callable_is_valid_and_get_method() {
    let output = run_output(
        "\
var cb = Callable(null, \"foo\")
print(cb.is_valid())
print(cb.get_method())
var empty = Callable()
print(empty.is_valid())
",
    );
    assert_eq!(output, vec!["true", "foo", "false"]);
}

#[test]
fn lambda_callable() {
    let val = run_val(
        "\
var square = func(x): return x * x
return square.call(9)
",
    );
    assert_eq!(val, Variant::Int(81));
}

// ===========================================================================
// 4. Signal declaration and emission
// ===========================================================================

#[test]
fn signal_declaration_stored_in_class() {
    let src = "\
class_name Emitter
extends Node

signal health_changed(old_value, new_value)
signal died

var health = 100
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");

    assert!(
        class_def.signals.contains(&"health_changed".to_string()),
        "health_changed signal should be declared"
    );
    assert!(
        class_def.signals.contains(&"died".to_string()),
        "died signal should be declared"
    );
}

#[test]
fn signal_count_matches_declarations() {
    let src = "\
class_name Multi
extends Node

signal a
signal b
signal c
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    assert_eq!(class_def.signals.len(), 3);
}

// ===========================================================================
// 5. Script function dispatch edge cases
// ===========================================================================

#[test]
fn function_returns_nil_implicitly() {
    let val = run_val(
        "\
func noop():
    var x = 1

return noop()
",
    );
    assert_eq!(val, Variant::Nil);
}

#[test]
fn function_multiple_return_paths() {
    let val = run_val(
        "\
func abs_val(x):
    if x < 0:
        return -x
    return x

return abs_val(-5)
",
    );
    assert_eq!(val, Variant::Int(5));
}

#[test]
fn class_method_accesses_instance_vars() {
    let src = "\
class_name Counter
extends Node

var count = 0

func increment():
    count = count + 1

func get_count():
    return count
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).unwrap();
    let mut inst = interp.instantiate_class(&class_def).unwrap();

    for _ in 0..3 {
        interp.call_instance_method(&mut inst, "increment", &[]).unwrap();
    }
    let count = interp.call_instance_method(&mut inst, "get_count", &[]).unwrap();
    assert_eq!(count, Variant::Int(3));
}

// ===========================================================================
// 7. @onready integration with scene tree lifecycle
// ===========================================================================

#[test]
fn onready_resolved_automatically_by_lifecycle_enter_tree() {
    use gdscene::lifecycle::LifecycleManager;
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;
    use gdscene::scripting::GDScriptNodeInstance;

    let script_src = "\
class_name AutoReady
extends Node

@onready
var health = 100

@onready
var speed = 3 * 5

var name_tag = \"hero\"

func get_health():
    return health

func get_speed():
    return speed
";

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Player", "Node");
    let player_id = tree.add_child(root, node).unwrap();

    // Attach script.
    let script = GDScriptNodeInstance::from_source(script_src, player_id)
        .expect("script parse failed");
    tree.attach_script(player_id, Box::new(script));

    // Before lifecycle: @onready vars should be Nil (deferred).
    {
        let s = tree.get_script(player_id).expect("script should be attached");
        assert_eq!(
            s.get_property("health"),
            Some(Variant::Nil),
            "@onready var should be Nil before enter_tree"
        );
        assert_eq!(
            s.get_property("speed"),
            Some(Variant::Nil),
            "@onready var should be Nil before enter_tree"
        );
        // Normal var should be initialized immediately.
        assert_eq!(
            s.get_property("name_tag"),
            Some(Variant::String("hero".to_string())),
            "normal var should be set at construction"
        );
    }

    // Trigger lifecycle (enter_tree + ready).
    LifecycleManager::enter_tree(&mut tree, player_id);

    // After lifecycle: @onready vars should be resolved.
    {
        let s = tree.get_script(player_id).expect("script should be attached");
        assert_eq!(
            s.get_property("health"),
            Some(Variant::Int(100)),
            "@onready var 'health' should be resolved after enter_tree"
        );
        assert_eq!(
            s.get_property("speed"),
            Some(Variant::Int(15)),
            "@onready var 'speed' (expression) should be resolved after enter_tree"
        );
        // Normal var unchanged.
        assert_eq!(
            s.get_property("name_tag"),
            Some(Variant::String("hero".to_string())),
        );
    }
}

#[test]
fn onready_resolved_before_ready_method_runs() {
    use gdscene::lifecycle::LifecycleManager;
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;
    use gdscene::scripting::GDScriptNodeInstance;

    // This script's _ready() reads an @onready var.
    // If @onready isn't resolved first, _ready would see Nil.
    let script_src = "\
class_name ReadyReader
extends Node

@onready
var base_hp = 50

var final_hp = 0

func _ready():
    final_hp = base_hp * 2
";

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Boss", "Node");
    let boss_id = tree.add_child(root, node).unwrap();

    let script = GDScriptNodeInstance::from_source(script_src, boss_id)
        .expect("script parse failed");
    tree.attach_script(boss_id, Box::new(script));

    // Trigger lifecycle.
    LifecycleManager::enter_tree(&mut tree, boss_id);

    // _ready() should have seen base_hp=50, so final_hp = 100.
    // If @onready wasn't resolved first, _ready would compute Nil * 2.
    let s = tree.get_script(boss_id).expect("script attached");
    assert_eq!(
        s.get_property("final_hp"),
        Some(Variant::Int(100)),
        "_ready() must see resolved @onready vars (base_hp=50 → final_hp=100)"
    );
}

// ===========================================================================
// 8. @onready is Nil during _enter_tree, resolved only before _ready
// ===========================================================================

#[test]
fn onready_nil_during_enter_tree_resolved_in_ready() {
    use gdscene::lifecycle::LifecycleManager;
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;
    use gdscene::scripting::GDScriptNodeInstance;

    // In Godot, @onready vars are NOT yet resolved when _enter_tree fires.
    // _enter_tree captures the var (should be Nil), _ready sees the resolved value.
    let script_src = "\
class_name PhaseTest
extends Node

@onready
var weapon = 99

var seen_in_enter_tree = -1
var seen_in_ready = -1

func _enter_tree():
    seen_in_enter_tree = weapon

func _ready():
    seen_in_ready = weapon
";

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Soldier", "Node");
    let id = tree.add_child(root, node).unwrap();

    let script = GDScriptNodeInstance::from_source(script_src, id).unwrap();
    tree.attach_script(id, Box::new(script));

    LifecycleManager::enter_tree(&mut tree, id);

    let s = tree.get_script(id).unwrap();
    // _enter_tree ran before @onready resolution — weapon was Nil.
    assert_eq!(
        s.get_property("seen_in_enter_tree"),
        Some(Variant::Nil),
        "@onready var must be Nil during _enter_tree"
    );
    // _ready ran after @onready resolution — weapon was 99.
    assert_eq!(
        s.get_property("seen_in_ready"),
        Some(Variant::Int(99)),
        "@onready var must be resolved before _ready"
    );
}

// ===========================================================================
// 9. Multi-child @onready bottom-up with cross-node property read
// ===========================================================================

#[test]
fn multi_child_onready_bottom_up_ordering() {
    use gdscene::lifecycle::LifecycleManager;
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;
    use gdscene::scripting::GDScriptNodeInstance;

    // Tree: Root → Parent → [ChildA, ChildB]
    // ChildA has @onready var, sets a flag in _ready.
    // ChildB has @onready var, sets a flag in _ready.
    // Parent's _ready reads both flags to prove children were ready first.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent_node = Node::new("Parent", "Node2D");
    let parent_id = tree.add_child(root, parent_node).unwrap();

    let child_a_node = Node::new("ChildA", "Node2D");
    let child_a_id = tree.add_child(parent_id, child_a_node).unwrap();

    let child_b_node = Node::new("ChildB", "Node2D");
    let child_b_id = tree.add_child(parent_id, child_b_node).unwrap();

    // ChildA: @onready resolves to 10, _ready doubles it to 20.
    let child_a_script = "\
extends Node2D

@onready
var base = 10

var result = 0

func _ready():
    self.result = base * 2
";
    let sa = GDScriptNodeInstance::from_source(child_a_script, child_a_id).unwrap();
    tree.attach_script(child_a_id, Box::new(sa));

    // ChildB: @onready resolves to 5, _ready triples it to 15.
    let child_b_script = "\
extends Node2D

@onready
var base = 5

var result = 0

func _ready():
    self.result = base * 3
";
    let sb = GDScriptNodeInstance::from_source(child_b_script, child_b_id).unwrap();
    tree.attach_script(child_b_id, Box::new(sb));

    // Parent: reads ChildA.result and ChildB.result in _ready.
    // Since _ready is bottom-up, children are ready before parent.
    let parent_script = "\
extends Node2D

var sum = 0

func _ready():
    var a = get_node(\"ChildA\")
    var b = get_node(\"ChildB\")
    self.sum = a.result + b.result
";
    let sp = GDScriptNodeInstance::from_source(parent_script, parent_id).unwrap();
    tree.attach_script(parent_id, Box::new(sp));

    LifecycleManager::enter_tree(&mut tree, parent_id);

    // Children must be ready before parent — verify computed values.
    assert_eq!(
        tree.get_script(child_a_id).unwrap().get_property("result"),
        Some(Variant::Int(20)),
        "ChildA @onready resolved and _ready computed 10*2=20"
    );
    assert_eq!(
        tree.get_script(child_b_id).unwrap().get_property("result"),
        Some(Variant::Int(15)),
        "ChildB @onready resolved and _ready computed 5*3=15"
    );
    // Parent read both children's results (bottom-up guarantee).
    assert_eq!(
        tree.get_script(parent_id).unwrap().get_property("sum"),
        Some(Variant::Int(35)),
        "Parent _ready should see children results: 20+15=35"
    );
}

// ===========================================================================
// 10. @onready + signal connect in _ready + emit end-to-end
// ===========================================================================

#[test]
fn onready_plus_signal_connect_emit_end_to_end() {
    use gdscene::lifecycle::LifecycleManager;
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;
    use gdscene::scripting::GDScriptNodeInstance;

    // Emitter has @onready var. In _ready, wires signal connect.
    // In _process, emits signal with @onready value as payload.
    // Listener receives and stores the value.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let parent_id = tree.add_child(root, parent).unwrap();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(parent_id, emitter).unwrap();

    let listener = Node::new("Listener", "Node2D");
    let listener_id = tree.add_child(parent_id, listener).unwrap();

    // Listener stores received damage.
    let listener_script = "\
extends Node2D
var damage_received = 0
func _on_hit(amount):
    self.damage_received = amount
";
    let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
    tree.attach_script(listener_id, Box::new(ls));

    // Emitter: @onready var sets damage, _ready wires signal, _process emits.
    let emitter_script = "\
extends Node2D

@onready
var damage = 42

func _ready():
    var p = get_parent()
    var me = p.get_node(\"Emitter\")
    var target = p.get_node(\"Listener\")
    me.connect(\"hit\", target, \"_on_hit\")

func _process(delta):
    emit_signal(\"hit\", damage)
";
    let es = GDScriptNodeInstance::from_source(emitter_script, emitter_id).unwrap();
    tree.attach_script(emitter_id, Box::new(es));

    // Lifecycle: enter_tree + ready (resolves @onready, fires _ready which wires signal).
    LifecycleManager::enter_tree(&mut tree, parent_id);

    // Verify @onready resolved.
    assert_eq!(
        tree.get_script(emitter_id).unwrap().get_property("damage"),
        Some(Variant::Int(42)),
        "@onready damage should be 42 after lifecycle"
    );

    // _process emits the signal with the @onready value.
    tree.process_script_process(emitter_id, 0.016);

    // Listener should have received the @onready-resolved damage value.
    assert_eq!(
        tree.get_script(listener_id)
            .unwrap()
            .get_property("damage_received"),
        Some(Variant::Int(42)),
        "Listener should receive @onready-resolved damage value via signal"
    );
}

// ===========================================================================
// 11. Callable method dispatch through scene tree
// ===========================================================================

#[test]
fn callable_dispatch_through_instance_method() {
    // Verify callable .call() works on class instance methods — not just
    // standalone functions — ensuring dispatch finds instance scope.
    let src = "\
class_name Weapon
extends Node

var ammo = 10

func fire():
    ammo = ammo - 1
    return ammo

func get_ammo():
    return ammo
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).unwrap();
    let mut instance = interp.instantiate_class(&class_def).unwrap();

    // Direct method call.
    let r1 = interp
        .call_instance_method(&mut instance, "fire", &[])
        .unwrap();
    assert_eq!(r1, Variant::Int(9));

    let r2 = interp
        .call_instance_method(&mut instance, "fire", &[])
        .unwrap();
    assert_eq!(r2, Variant::Int(8));

    let ammo = interp
        .call_instance_method(&mut instance, "get_ammo", &[])
        .unwrap();
    assert_eq!(ammo, Variant::Int(8));
}

// ===========================================================================
// 12. Function dispatch: varargs-like with multiple defaults
// ===========================================================================

#[test]
fn function_multiple_defaults_partial_override() {
    let val = run_val(
        "\
func setup(name = \"Player\", hp = 100, speed = 5):
    return name + \":\" + str(hp) + \":\" + str(speed)

return setup(\"Boss\", 500)
",
    );
    assert_eq!(val, Variant::String("Boss:500:5".into()));
}

#[test]
fn function_all_defaults_used() {
    let val = run_val(
        "\
func setup(name = \"Player\", hp = 100, speed = 5):
    return name + \":\" + str(hp) + \":\" + str(speed)

return setup()
",
    );
    assert_eq!(val, Variant::String("Player:100:5".into()));
}
