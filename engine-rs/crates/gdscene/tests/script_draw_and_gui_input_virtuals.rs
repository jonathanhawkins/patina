//! pat-0q2kx.82: the Object/CanvasItem virtual chain a GDScript can override.
//!
//! Godot dispatches far more than the eleven lifecycle/input callbacks Patina
//! wired. Three families were missing entirely, each with a live call path in
//! upstream Godot and none here:
//!
//! - `queue_redraw()` + `_draw()` (scene/main/canvas_item.cpp): queuing a
//!   redraw runs `_draw` in a context where the `draw_*` calls are valid, and
//!   the commands they emit become the item's drawing for the frame. Patina had
//!   no `queue_redraw` builtin at all, so a script whose entire visual output is
//!   `_draw` rendered nothing.
//! - `_to_string()` (core/object/object.cpp `Object::to_string`): what `str()`
//!   and `print()` must show for an object that overrides it.
//! - `_get` / `_set` / `_get_property_list` (the `Object::get`/`Object::set`
//!   tail): dynamic properties that exist only in the script's own bookkeeping.
//!
//! `_gui_input` is dispatched by the GUI stage (pat-0q2kx.18); what this bead
//! adds there is `accept_event()`, the documented way a handler stops the event
//! from bubbling to ancestor Controls.
//!
//! Every assertion below is on script-observable or renderer-observable state —
//! a draw command that reaches the canvas viewport, a counter a handler bumped —
//! so "the machinery exists but nothing calls it" fails.

use gdcore::math::{Color, Vector2};
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::node::{Node, NodeId};
use gdscene::scene_bridge::build_canvas_viewport;
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdserver2d::canvas::DrawCommand;
use gdvariant::Variant;

/// Reads a script variable back off the node's script instance.
fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Option<Variant> {
    tree.get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// Every draw command the canvas bridge produces for the whole tree, flattened.
/// Going through [`build_canvas_viewport`] rather than reading the tree's
/// stored commands is the point: it proves `_draw` output actually reaches the
/// render list, not just a side table nothing consumes.
fn rendered_commands(tree: &SceneTree) -> Vec<DrawCommand> {
    let no_textures = |_p: &str| -> Option<Vector2> { None };
    let viewport =
        build_canvas_viewport(tree, 200, 200, Color::new(0.0, 0.0, 0.0, 1.0), &no_textures);
    viewport
        .get_sorted_items()
        .into_iter()
        .flat_map(|item| item.commands.iter().cloned())
        .collect()
}

fn attach(tree: &mut SceneTree, node: NodeId, source: &str) {
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(source, node).unwrap()),
    );
}

fn press(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(x, y),
    }
}

/// A Control node with an explicit rect, as the GUI hit test expects.
fn control(name: &str, class: &str, x: f32, y: f32, w: f32, h: f32) -> Node {
    let mut n = Node::new(name, class);
    n.set_property("position", Variant::Vector2(Vector2::new(x, y)));
    n.set_property("size", Variant::Vector2(Vector2::new(w, h)));
    n
}

/// **The acceptance test.** All three virtuals the bead names, each asserted on
/// the observable effect rather than on the presence of a call site.
#[test]
fn script_draw_and_gui_input_virtuals() {
    // -- (1) `_draw` + `queue_redraw` reach the render list ------------------
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let painter = tree
        .add_child(root, Node::new("Painter", "Node2D"))
        .unwrap();
    attach(
        &mut tree,
        painter,
        "\
extends Node2D
func _ready():
    queue_redraw()
func _draw():
    draw_rect(Rect2(10, 20, 30, 40), Color(1, 0, 0, 1), true)
",
    );
    let _ = tree.call_script_method(painter, "_ready", &[]);
    assert!(
        tree.is_queued_for_redraw(painter),
        "queue_redraw() from _ready must mark the item dirty"
    );

    tree.flush_pending_redraws();
    let commands = rendered_commands(&tree);
    let drawn = commands
        .iter()
        .find(|c| matches!(c, DrawCommand::DrawRect { .. }))
        .unwrap_or_else(|| {
            panic!("the script's _draw rect must reach the canvas viewport, got {commands:?}")
        });
    match drawn {
        DrawCommand::DrawRect {
            rect,
            color,
            filled,
        } => {
            assert_eq!(rect.position, Vector2::new(10.0, 20.0));
            assert_eq!(rect.size, Vector2::new(30.0, 40.0));
            assert_eq!(*color, Color::new(1.0, 0.0, 0.0, 1.0));
            assert!(*filled, "draw_rect(.., filled = true) must fill");
        }
        other => panic!("expected DrawRect, got {other:?}"),
    }

    // -- (2) `_gui_input` + `accept_event()` stops propagation ---------------
    //
    // A MOUSE_FILTER_PASS child normally bubbles to its ancestor Control
    // (pat-0q2kx.18). `accept_event()` in the child's handler must end the walk,
    // so the parent's `_gui_input` never runs.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let panel = tree
        .add_child(root, control("Panel", "Panel", 0.0, 0.0, 200.0, 200.0))
        .unwrap();
    let mut inner = control("Inner", "ColorRect", 10.0, 10.0, 50.0, 50.0);
    // MouseFilter::Pass == 1 (Godot MOUSE_FILTER_PASS) — without accept_event
    // this child bubbles.
    inner.set_property("mouse_filter", Variant::Int(1));
    let inner = tree.add_child(panel, inner).unwrap();

    attach(
        &mut tree,
        panel,
        "\
extends Control
var gui_events = 0
func _gui_input(event):
    self.gui_events = self.gui_events + 1
",
    );
    attach(
        &mut tree,
        inner,
        "\
extends Control
var gui_events = 0
func _gui_input(event):
    self.gui_events = self.gui_events + 1
    accept_event()
",
    );

    tree.push_input(&press(20.0, 20.0));

    assert!(
        matches!(script_var(&tree, inner, "gui_events"), Some(Variant::Int(n)) if n >= 1),
        "the hit control's _gui_input must run, got {:?}",
        script_var(&tree, inner, "gui_events")
    );
    assert_eq!(
        script_var(&tree, panel, "gui_events"),
        Some(Variant::Int(0)),
        "accept_event() in the child's _gui_input must stop the event before it \
         bubbles to the PASS ancestor"
    );

    // -- (3) `str(obj)` consults `_to_string` --------------------------------
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let target = tree.add_child(root, Node::new("Target", "Node2D")).unwrap();
    let reader = tree.add_child(root, Node::new("Reader", "Node2D")).unwrap();
    attach(
        &mut tree,
        target,
        "\
extends Node2D
func _to_string():
    return \"Player(hp=10)\"
",
    );
    attach(
        &mut tree,
        reader,
        "\
extends Node2D
var seen = \"\"
func _ready():
    self.seen = str(get_node(\"../Target\"))
",
    );
    let _ = tree.call_script_method(reader, "_ready", &[]);
    assert_eq!(
        script_var(&tree, reader, "seen"),
        Some(Variant::String("Player(hp=10)".to_string())),
        "str(obj) must return the object's _to_string() override"
    );
}

/// `_to_string` also answers for `self` and for `print()`. `self` is the harder
/// case: the running script is checked out of the tree's script arena, so it
/// can only be reached through the interpreter, not the scene accessor.
#[test]
fn to_string_override_answers_for_self_and_print() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Thing", "Node2D")).unwrap();
    attach(
        &mut tree,
        node,
        "\
extends Node2D
var seen = \"\"
func _ready():
    self.seen = str(self)
func _to_string():
    return \"Thing#7\"
",
    );
    let _ = tree.call_script_method(node, "_ready", &[]);
    assert_eq!(
        script_var(&tree, node, "seen"),
        Some(Variant::String("Thing#7".to_string())),
        "str(self) must consult the running script's own _to_string"
    );
}

/// An object with no `_to_string` keeps the engine's default representation —
/// the override must not be invented for every object.
#[test]
fn without_the_override_str_keeps_the_default_form() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Plain", "Node2D")).unwrap();
    attach(
        &mut tree,
        node,
        "\
extends Node2D
var seen = \"\"
func _ready():
    self.seen = str(self)
",
    );
    let _ = tree.call_script_method(node, "_ready", &[]);
    let seen = script_var(&tree, node, "seen");
    assert!(
        !matches!(&seen, Some(Variant::String(s)) if s.is_empty()),
        "str(self) must still produce something without _to_string, got {seen:?}"
    );
}

/// A redraw is consumed by the pass that runs it: `_draw` must not re-run every
/// frame on its own. This is what makes `queue_redraw` load-bearing rather than
/// decorative — without the pending-set bookkeeping, `_draw` would either never
/// run or run unconditionally, and both would pass a naive "is the rect there?"
/// assertion.
#[test]
fn draw_runs_once_per_queued_redraw() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Painter", "Node2D"))
        .unwrap();
    attach(
        &mut tree,
        node,
        "\
extends Node2D
var draws = 0
func _draw():
    self.draws = self.draws + 1
    draw_circle(Vector2(5, 5), 3.0, Color(0, 1, 0, 1))
",
    );

    // Attaching a script that overrides `_draw` queues the first redraw, as
    // Godot does when the item enters the canvas.
    tree.flush_pending_redraws();
    assert_eq!(script_var(&tree, node, "draws"), Some(Variant::Int(1)));

    // A second pass with nothing queued must not run `_draw` again...
    tree.flush_pending_redraws();
    assert_eq!(
        script_var(&tree, node, "draws"),
        Some(Variant::Int(1)),
        "_draw must run only when a redraw is queued, not on every pass"
    );
    // ...but the commands from the last redraw survive, as Godot keeps a
    // CanvasItem's command list alive until it is redrawn.
    assert!(
        rendered_commands(&tree)
            .iter()
            .any(|c| matches!(c, DrawCommand::DrawCircle { .. })),
        "commands from the previous redraw must persist across frames"
    );

    // Queuing again re-runs it.
    tree.queue_redraw(node);
    tree.flush_pending_redraws();
    assert_eq!(
        script_var(&tree, node, "draws"),
        Some(Variant::Int(2)),
        "queue_redraw() must schedule another _draw"
    );
}

/// Godot only honors `draw_*` inside `_draw`. A `draw_rect` from `_process`
/// draws nothing, and must not leak into the next redraw's command list.
#[test]
fn draw_calls_outside_draw_are_dropped() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Painter", "Node2D"))
        .unwrap();
    attach(
        &mut tree,
        node,
        "\
extends Node2D
func _process(delta):
    draw_rect(Rect2(0, 0, 10, 10), Color(1, 0, 0, 1), true)
",
    );

    tree.process_script_process(node, 0.016);
    tree.flush_pending_redraws();

    assert!(
        rendered_commands(&tree).is_empty(),
        "a draw_* call outside _draw must be dropped, not queued for the next pass"
    );
}

/// The multi-command case, and the one that proves the bridge appends rather
/// than replaces: a `_draw` on a `ColorRect` overlays the rect's own fill.
#[test]
fn custom_draw_appends_to_the_nodes_own_drawing() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut rect = Node::new("Backdrop", "ColorRect");
    rect.set_property("position", Variant::Vector2(Vector2::ZERO));
    rect.set_property("size", Variant::Vector2(Vector2::new(64.0, 64.0)));
    rect.set_property("color", Variant::Color(Color::new(0.0, 0.0, 1.0, 1.0)));
    let rect = tree.add_child(root, rect).unwrap();
    attach(
        &mut tree,
        rect,
        "\
extends ColorRect
func _draw():
    draw_line(Vector2(0, 0), Vector2(64, 64), Color(1, 1, 0, 1), 2.0)
    draw_string(null, Vector2(4, 12), \"hi\", 0, -1, 24, Color(1, 1, 1, 1))
",
    );
    tree.flush_pending_redraws();

    let commands = rendered_commands(&tree);
    assert!(
        commands.len() >= 3,
        "the ColorRect's own fill plus both _draw commands must be present, got {commands:?}"
    );
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, DrawCommand::DrawLine { width, .. } if *width == 2.0)),
        "draw_line must carry its width through, got {commands:?}"
    );
    assert!(
        commands.iter().any(
            |c| matches!(c, DrawCommand::DrawString { text, font_size, .. } if text == "hi" && *font_size == 24)
        ),
        "draw_string must carry its text and font_size through, got {commands:?}"
    );
}

/// `_get` / `_set`: properties that exist only in the script's own bookkeeping.
/// Godot consults `_get` only after the real property lookup misses, and offers
/// `_set` only names the object does not already carry — so a real property
/// must NOT be routed through the virtuals.
#[test]
fn get_and_set_virtuals_back_dynamic_properties() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let holder = tree.add_child(root, Node::new("Holder", "Node2D")).unwrap();
    let driver = tree.add_child(root, Node::new("Driver", "Node2D")).unwrap();

    attach(
        &mut tree,
        holder,
        "\
extends Node2D
var real_prop = 1
var dynamic_store = 0
var set_calls = 0
func _get(property):
    if property == \"virtual_prop\":
        return self.dynamic_store
    return null
func _set(property, value):
    if property == \"virtual_prop\":
        self.dynamic_store = value
        self.set_calls = self.set_calls + 1
        return true
    return false
",
    );
    attach(
        &mut tree,
        driver,
        "\
extends Node2D
var read_back = 0
var real_read = 0
func _ready():
    var holder = get_node(\"../Holder\")
    holder.virtual_prop = 42
    self.read_back = holder.virtual_prop
    self.real_read = holder.real_prop
",
    );
    let _ = tree.call_script_method(driver, "_ready", &[]);

    assert_eq!(
        script_var(&tree, driver, "read_back"),
        Some(Variant::Int(42)),
        "a write to an unknown property must route through _set, and the read \
         back through _get"
    );
    assert_eq!(
        script_var(&tree, holder, "dynamic_store"),
        Some(Variant::Int(42)),
        "_set must have stored the value in the script's own bookkeeping"
    );
    assert_eq!(
        script_var(&tree, driver, "real_read"),
        Some(Variant::Int(1)),
        "a property the object really has must be read directly, not via _get"
    );
    assert_eq!(
        script_var(&tree, holder, "set_calls"),
        Some(Variant::Int(1)),
        "_set must fire exactly once for the one unknown-property write"
    );
}

/// `_get_property_list` entries are appended to the class's own, which is how a
/// script publishes dynamic properties to reflection and the inspector.
#[test]
fn get_property_list_merges_the_script_virtual() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let holder = tree.add_child(root, Node::new("Holder", "Node2D")).unwrap();
    let driver = tree.add_child(root, Node::new("Driver", "Node2D")).unwrap();

    attach(
        &mut tree,
        holder,
        "\
extends Node2D
func _get_property_list():
    return [{\"name\": \"virtual_prop\", \"type\": 2}]
",
    );
    attach(
        &mut tree,
        driver,
        "\
extends Node2D
var found = false
func _ready():
    var holder = get_node(\"../Holder\")
    for entry in holder.get_property_list():
        if entry[\"name\"] == \"virtual_prop\":
            self.found = true
",
    );
    let _ = tree.call_script_method(driver, "_ready", &[]);

    assert_eq!(
        script_var(&tree, driver, "found"),
        Some(Variant::Bool(true)),
        "get_property_list() must include the entries _get_property_list reports"
    );
}
