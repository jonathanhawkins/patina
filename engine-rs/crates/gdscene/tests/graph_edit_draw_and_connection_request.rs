//! pat-s3rrv.233 — GraphEdit draws its graph and connections can be made by hand.
//!
//! The connection MODEL already existed: `graph_edit.rs` is a complete 732-line
//! implementation, stored per-node on the tree and reachable from GDScript for
//! `connect_node` / `disconnect_node` / `get_connection_list`.
//!
//! Nothing drew it and nothing drove it. `draw_control` had no `GraphEdit` arm, so
//! a graph was invisible; `gui_input_stage` had no port-drag branch, so no
//! connection could be made by hand and `connection_request` / `node_selected`
//! were never emitted. A GraphEdit was a data structure with no way in or out.
//!
//! ## The invariant these tests exist to protect
//!
//! A port dot and the end of a connection line are ONE quantity. They are both
//! produced by `graph_port_position`, and the test below asserts the polyline's
//! first point equals the output dot's centre. If the two were ever computed
//! separately, a line could point into empty space and every frame would still
//! look entirely plausible — the kind of bug a screenshot cannot catch.

use gdcore::id::ObjectId;
use gdcore::math::{Color, Vector2};
use gdobject::class_db;
use gdobject::signal::Connection;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::render_control_tree;
use gdscene::graph_edit::GraphNode as GraphNodeModel;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;
use std::sync::{Arc, Mutex};

/// Node A sits here, node B to its right and below.
const A_OFFSET: (f32, f32) = (20.0, 20.0);
const B_OFFSET: (f32, f32) = (200.0, 90.0);
const NODE_SIZE: (f32, f32) = (120.0, 60.0);
/// Mirrors `control_render`'s own constants; duplicated deliberately so a silent
/// change to the layout there fails here rather than quietly moving every port.
const TITLEBAR_H: f32 = 20.0;
const PORT_PITCH: f32 = 18.0;

/// Where port `port` of the node at `offset` should be.
fn expected_port(offset: (f32, f32), port: u32, is_output: bool) -> Vector2 {
    let x = if is_output {
        offset.0 + NODE_SIZE.0
    } else {
        offset.0
    };
    Vector2::new(x, offset.1 + TITLEBAR_H + PORT_PITCH * (port as f32 + 0.5))
}

/// A GraphEdit with two GraphNodes: A has one output, B has one input.
fn graph_scene() -> (SceneTree, NodeId, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let edit = tree
        .add_child(root, Node::new("Graph", "GraphEdit"))
        .unwrap();
    control::apply_anchor_preset(&mut tree, edit, AnchorPreset::TopLeft);
    control::set_offset_right(&mut tree, edit, 400.0);
    control::set_offset_bottom(&mut tree, edit, 240.0);

    let mut add = |tree: &mut SceneTree, name: &str, off: (f32, f32)| {
        let mut n = Node::new(name, "GraphNode");
        n.set_property(
            "position_offset",
            Variant::Vector2(Vector2::new(off.0, off.1)),
        );
        n.set_property(
            "size",
            Variant::Vector2(Vector2::new(NODE_SIZE.0, NODE_SIZE.1)),
        );
        tree.add_child(edit, n).unwrap()
    };
    let a = add(&mut tree, "A", A_OFFSET);
    let b = add(&mut tree, "B", B_OFFSET);

    // The slot model: A exposes one output, B one input.
    let mut ma = GraphNodeModel::new("Alpha");
    ma.set_output_slot(0, 0);
    let mut mb = GraphNodeModel::new("Beta");
    mb.set_input_slot(0, 0);
    let model = tree.graph_edit_mut(edit);
    model.add_node("A", ma);
    model.add_node("B", mb);

    (tree, edit, a, b)
}

fn draw(tree: &mut SceneTree, edit: NodeId) -> Vec<DrawCommand> {
    let mut vp = Viewport::new(400, 240, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(tree, edit, &theme, &mut vp, Vector2::new(400.0, 240.0));
    vp.get_sorted_items()
        .iter()
        .flat_map(|it| it.commands.clone())
        .collect()
}

fn circles(cmds: &[DrawCommand]) -> Vec<Vector2> {
    cmds.iter()
        .filter_map(|c| match c {
            DrawCommand::DrawCircle { center, .. } => Some(*center),
            _ => None,
        })
        .collect()
}

fn strings(cmds: &[DrawCommand]) -> Vec<String> {
    cmds.iter()
        .filter_map(|c| match c {
            DrawCommand::DrawString { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn polylines(cmds: &[DrawCommand]) -> Vec<Vec<Vector2>> {
    cmds.iter()
        .filter_map(|c| match c {
            DrawCommand::DrawPolyline { points, .. } => Some(points.clone()),
            _ => None,
        })
        .collect()
}

fn has_rect_at(cmds: &[DrawCommand], pos: Vector2, size: Vector2) -> bool {
    cmds.iter().any(|c| match c {
        DrawCommand::DrawRect { rect, .. } => {
            (rect.position.x - pos.x).abs() < 0.5
                && (rect.position.y - pos.y).abs() < 0.5
                && (rect.size.x - size.x).abs() < 0.5
                && (rect.size.y - size.y).abs() < 0.5
        }
        _ => false,
    })
}

/// Records a signal's arguments. Mirrors `range_control_family_renders_and_drags`
/// rather than inventing a second recorder.
///
/// ⚠️ `tag` must be HIGH and unused, not merely unique. It becomes
/// `ObjectId::from_raw(tag)`, and a low value collides with a real node's id —
/// this scene's root/GraphEdit/A/B occupy the small ids — which routes the
/// connection at a live object instead of at the callback. The reference test
/// uses 69 for the same reason; these use 9000+.
fn watch(
    tree: &mut SceneTree,
    node: NodeId,
    signal: &str,
    tag: u64,
) -> Arc<Mutex<Vec<Vec<Variant>>>> {
    let log: Arc<Mutex<Vec<Vec<Variant>>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    tree.connect_signal(
        node,
        signal,
        Connection::with_callback(ObjectId::from_raw(tag), "on_signal", move |args| {
            sink.lock().unwrap().push(args.to_vec());
            Variant::Nil
        }),
    );
    log
}

/// **The acceptance.** Both nodes draw, a hand-made port drag emits
/// `connection_request`, and once the graph accepts it the line appears.
#[test]
fn graph_edit_draw_and_connection_request() {
    let (mut tree, edit, _a, _b) = graph_scene();

    // --- both node panels, their titles and their port dots ----------------
    let cmds = draw(&mut tree, edit);
    assert!(
        has_rect_at(
            &cmds,
            Vector2::new(A_OFFSET.0, A_OFFSET.1),
            Vector2::new(NODE_SIZE.0, NODE_SIZE.1)
        ),
        "node A's panel must be drawn at its position_offset"
    );
    assert!(
        has_rect_at(
            &cmds,
            Vector2::new(B_OFFSET.0, B_OFFSET.1),
            Vector2::new(NODE_SIZE.0, NODE_SIZE.1)
        ),
        "node B's panel must be drawn at its position_offset"
    );

    let titles = strings(&cmds);
    assert!(
        titles.contains(&"Alpha".to_string()) && titles.contains(&"Beta".to_string()),
        "both titles must be drawn, got {titles:?}"
    );

    let dots = circles(&cmds);
    let a_out = expected_port(A_OFFSET, 0, true);
    let b_in = expected_port(B_OFFSET, 0, false);
    let near = |v: Vector2, w: Vector2| (v.x - w.x).abs() < 0.5 && (v.y - w.y).abs() < 0.5;
    assert!(
        dots.iter().any(|d| near(*d, a_out)),
        "A's output port dot must be at {a_out:?}, got {dots:?}"
    );
    assert!(
        dots.iter().any(|d| near(*d, b_in)),
        "B's input port dot must be at {b_in:?}, got {dots:?}"
    );

    // Nothing is connected yet, so there must be no line. Without this control
    // the assertion further down proves only that SOME polyline exists.
    assert!(
        polylines(&cmds).is_empty(),
        "no connection exists yet, so no connection line may be drawn"
    );

    // --- dragging A's output onto B's input asks for a connection ----------
    let seen = watch(&mut tree, edit, "connection_request", 9001);

    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: a_out,
    });
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: b_in,
    });

    let got = seen.lock().unwrap().clone();
    assert_eq!(
        got.len(),
        1,
        "exactly one connection_request must be emitted, got {}",
        got.len()
    );
    assert_eq!(
        got[0],
        vec![
            Variant::String("A".to_string()),
            Variant::Int(0),
            Variant::String("B".to_string()),
            Variant::Int(0),
        ],
        "connection_request carries (from, from_port, to, to_port)"
    );

    // --- the graph accepts it, and the next frame draws the line -----------
    //
    // The engine emits a REQUEST and connects nothing itself; that is what lets a
    // game refuse a connection. So the accept step is explicit here too.
    tree.graph_edit_mut(edit)
        .connect_node("A", 0, "B", 0)
        .expect("the requested connection is valid");

    let cmds = draw(&mut tree, edit);
    let lines = polylines(&cmds);
    assert_eq!(lines.len(), 1, "one connection, one line");
    let line = &lines[0];
    assert!(
        near(line[0], a_out),
        "the line must START at A's output DOT — dots and lines are one quantity, \
         and a line that begins anywhere else is pointing at nothing. got {:?} want {a_out:?}",
        line[0]
    );
    assert!(
        near(line[line.len() - 1], b_in),
        "and END at B's input dot. got {:?} want {b_in:?}",
        line[line.len() - 1]
    );
}

/// Dragging a titlebar moves the node and announces the selection.
#[test]
fn graph_edit_draw_and_connection_request_titlebar_drag_moves_and_selects() {
    let (mut tree, edit, a, _b) = graph_scene();

    let selected = watch(&mut tree, edit, "node_selected", 9002);

    // Grab A's titlebar off-centre, so a handler that snapped the corner to the
    // cursor would land somewhere different from one that keeps the grab offset.
    let grab = Vector2::new(A_OFFSET.0 + 30.0, A_OFFSET.1 + 8.0);
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: grab,
    });
    assert_eq!(
        selected.lock().unwrap().len(),
        1,
        "pressing a node's titlebar must emit node_selected"
    );

    tree.push_input(&InputEvent::MouseMotion {
        position: Vector2::new(grab.x + 40.0, grab.y + 25.0),
        relative: Vector2::new(40.0, 25.0),
    });

    let off = match tree.get_node(a).map(|n| n.get_property("position_offset")) {
        Some(Variant::Vector2(v)) => v,
        other => panic!("position_offset must be a Vector2, got {other:?}"),
    };
    assert!(
        (off.x - (A_OFFSET.0 + 40.0)).abs() < 0.5 && (off.y - (A_OFFSET.1 + 25.0)).abs() < 0.5,
        "the node must move BY the drag delta, keeping the grabbed point under the \
         cursor — got {off:?}, want {:?}",
        Vector2::new(A_OFFSET.0 + 40.0, A_OFFSET.1 + 25.0)
    );

    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: Vector2::new(grab.x + 40.0, grab.y + 25.0),
    });
}

/// A drag that ends on empty canvas reports `connection_to_empty`, not a
/// connection — Godot uses it to offer a "create node here" menu.
#[test]
fn graph_edit_draw_and_connection_request_drag_to_empty_canvas() {
    let (mut tree, edit, _a, _b) = graph_scene();

    let empty = watch(&mut tree, edit, "connection_to_empty", 9003);
    let requests = watch(&mut tree, edit, "connection_request", 9004);

    // CONTROL FIRST: the same fixture, the same press, but released on a real
    // port. If this fails, the problem is the fixture or the press path, not the
    // empty-canvas branch — which is the distinction a bare "it did not fire"
    // cannot make.
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: expected_port(A_OFFSET, 0, true),
    });
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: expected_port(B_OFFSET, 0, false),
    });
    assert_eq!(
        requests.lock().unwrap().len(),
        1,
        "CONTROL: a port-to-port drag in this fixture must emit connection_request; \
         if this line fails the fixture or the press path is at fault, not the \
         empty-canvas branch"
    );

    // Now the case under test: same press, released over bare canvas.
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: expected_port(A_OFFSET, 0, true),
    });
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: Vector2::new(350.0, 210.0),
    });

    assert_eq!(
        empty.lock().unwrap().len(),
        1,
        "connection_to_empty must fire"
    );
    assert_eq!(
        requests.lock().unwrap().len(),
        1,
        "still just the control's connection — the empty-canvas release must NOT \
         add a second connection_request"
    );
}

/// **The regression guard for a test that passed for the wrong reason.**
///
/// Every other test here runs with an EMPTY ClassDB, where only the
/// `CONTROL_HIT_CLASSES` fallback matches and `control_at_point` therefore returns
/// the GraphEdit. Register the real classes — the editor's actual state — and
/// `GraphNode -> GraphElement -> Container -> Control` makes a GraphNode a Control
/// too, so the hit becomes the CHILD.
///
/// An implementation that checks the hit's own class passes all the other tests in
/// this file and does nothing in the real editor. Only this one can tell them
/// apart, which is exactly why it exists.
#[test]
fn graph_edit_draw_and_connection_request_resolves_the_edit_with_classdb_populated() {
    class_db::register_all_classes();
    assert!(
        class_db::is_parent_class("GraphNode", "Control"),
        "precondition: with ClassDB populated a GraphNode IS a Control, which is \
         what makes it steal the hit from its GraphEdit"
    );

    let (mut tree, edit, _a, _b) = graph_scene();
    let seen = watch(&mut tree, edit, "connection_request", 9005);
    let selected = watch(&mut tree, edit, "node_selected", 9006);

    // CONTROL: press a TITLEBAR first. That only emits node_selected if the press
    // resolved to the owning GraphEdit — i.e. if `enclosing_graph_edit` walked up
    // correctly from whatever ClassDB made `control_at_point` return. If this
    // fails, the walk-up is the problem; if it passes but the port drag below
    // fails, the problem is in the port path instead.
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(A_OFFSET.0 + 30.0, A_OFFSET.1 + 8.0),
    });
    assert_eq!(
        selected.lock().unwrap().len(),
        1,
        "CONTROL: with ClassDB populated a titlebar press must still resolve to \
         the GraphEdit and emit node_selected"
    );
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: Vector2::new(A_OFFSET.0 + 30.0, A_OFFSET.1 + 8.0),
    });

    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: expected_port(A_OFFSET, 0, true),
    });
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: expected_port(B_OFFSET, 0, false),
    });

    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "the port drag must still resolve to the owning GraphEdit when the hit \
         lands on a GraphNode child"
    );
}
