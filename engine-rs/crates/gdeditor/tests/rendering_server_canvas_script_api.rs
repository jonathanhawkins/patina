//! pat-0q2kx.213 — a script drives raw canvas items through the
//! `RenderingServer` singleton and they rasterize on the Play path.
//!
//! Godot ground truth: `servers/rendering_server.cpp`'s `canvas_*` bindings. A
//! script calls `canvas_item_create()` for an RID, parents it to its node with
//! `canvas_item_set_parent(ci, get_canvas_item())`, and appends draw commands
//! (`canvas_item_add_rect`, …) that render at the node's global transform.
//!
//! Patina had no `RenderingServer` in the interpreter at all, so
//! `RenderingServer.canvas_item_create()` did not resolve.
//!
//! ⚠️ The point of this file is the **wiring**, not the store. A canvas-item
//! store that scripts mutate but the rasterizer never reads would be the
//! systemic unwired failure this bead exists to avoid — so every assertion here
//! goes through the real Play-path calls (`build_runtime_render_list` +
//! `rasterize_render_list`) and looks at produced pixels.

use gdcore::math::Color;
use gdeditor::viewport_2d_render::{
    build_runtime_render_list, rasterize_render_list, RUNTIME_CLEAR_COLOR,
};
use gdrender2d::renderer::FrameBuffer;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::{GDScriptNodeInstance, MainLoop, NodeId, SceneTree};
use gdvariant::Variant;

const W: u32 = 320;
const H: u32 = 240;

/// The painter sits at (60, 120), so its 20x20 local rect covers
/// x ∈ [60, 80), y ∈ [120, 140) in canvas space.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Painter" type="Node2D" parent="."]
position = Vector2(60, 120)
"#;

/// Verbatim-shaped Godot: the acceptance sequence in `_ready`, plus one-line
/// methods the test calls to drive each state change through the *script* rather
/// than reaching into the store behind its back.
const PAINTER_GD: &str = "\
extends Node2D
var ci = 0
var ready_ran = false
func _ready():
    ci = RenderingServer.canvas_item_create()
    RenderingServer.canvas_item_set_parent(ci, get_canvas_item())
    RenderingServer.canvas_item_add_rect(ci, Rect2(0, 0, 20, 20), Color(1, 0, 0, 1))
    ready_ran = true
func hide_it():
    RenderingServer.canvas_item_set_visible(ci, false)
func show_it():
    RenderingServer.canvas_item_set_visible(ci, true)
func clear_it():
    RenderingServer.canvas_item_clear(ci)
";

fn same_color(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 0.01
        && (a.g - b.g).abs() < 0.01
        && (a.b - b.b).abs() < 0.01
        && (a.a - b.a).abs() < 0.01
}

/// The Play canvas frame — the two production calls the editor's Play tab makes.
fn play_frame(tree: &SceneTree) -> FrameBuffer {
    let items = build_runtime_render_list(tree);
    rasterize_render_list(&items, W, H, RUNTIME_CLEAR_COLOR, None)
}

/// Pixels matching `want`, so a count of zero is a real "nothing drew" rather
/// than "drew something else".
fn pixels_of(fb: &FrameBuffer, want: Color) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for y in 0..fb.height {
        for x in 0..fb.width {
            if same_color(fb.pixels[(y * fb.width + x) as usize], want) {
                out.push((x, y));
            }
        }
    }
    out
}

fn red() -> Color {
    Color::new(1.0, 0.0, 0.0, 1.0)
}

fn scripted_scene() -> (MainLoop, NodeId) {
    // The render list admits a node only when `is_class("CanvasItem")`, which
    // resolves through a ClassDB that starts EMPTY — without this the painter
    // never enters the list and every assertion below would be vacuous.
    gdobject::class_db::register_2d_classes();
    // The canvas store is a process global shared with every other test in this
    // binary; a neighbour's leftover items could otherwise satisfy these
    // assertions.
    gdscript_interop::canvas_server::reset();

    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let painter = tree
        .get_node_or_null(scene_root, "Painter")
        .expect("Painter node");
    tree.attach_script(
        painter,
        Box::new(GDScriptNodeInstance::from_source(PAINTER_GD, painter).unwrap()),
    );
    // `MainLoop::step` has no `_ready` phase — `process_script_ready` is the only
    // dispatch for it — so the setup would never run if the loop were merely
    // stepped. This is the same call a Play session makes on scene entry.
    tree.process_script_ready(painter);
    (MainLoop::new(tree), painter)
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    use gdscript_interop::bindings::ScriptInstance;
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// **The acceptance test.** Each clause of the bead is a separately-named link so
/// a failure says which one broke.
#[test]
fn rendering_server_canvas_script_api() {
    let (mut ml, painter) = scripted_scene();

    // `_ready` already ran the create/parent/add_rect sequence; one frame proves
    // the item survives a loop step rather than being scratch state.
    ml.step(1.0 / 60.0);

    // Link 0 — the handler ran to completion. An unresolved singleton method is
    // a RuntimeError that would abort `_ready` before this was set, so without
    // it the pixel assertions below would be misleading.
    assert_eq!(
        script_var(&ml, painter, "ready_ran"),
        Some(Variant::Bool(true)),
        "link 0: _ready must run to completion — a RenderingServer method that \
         does not resolve aborts it partway"
    );

    // Link 1 — canvas_item_create returned a real RID (Godot's invalid RID is 0).
    let ci = match script_var(&ml, painter, "ci") {
        Some(Variant::Int(i)) => i,
        other => panic!("link 1: `ci` should be an RID Int, got {other:?}"),
    };
    assert!(
        ci > 0,
        "link 1: canvas_item_create() must return a live RID, got {ci}"
    );

    // Link 2 — the commands rasterize on the Play path, at the NODE's global
    // position rather than the canvas origin.
    let frame = play_frame(ml.tree());
    let lit = pixels_of(&frame, red());
    assert!(
        !lit.is_empty(),
        "link 2: the script's canvas_item_add_rect must reach the Play frame — no \
         red pixels were drawn, so the store is not wired to the rasterizer"
    );
    let min_x = lit.iter().map(|p| p.0).min().unwrap();
    let max_x = lit.iter().map(|p| p.0).max().unwrap();
    let min_y = lit.iter().map(|p| p.1).min().unwrap();
    let max_y = lit.iter().map(|p| p.1).max().unwrap();
    assert!(
        (58..=62).contains(&min_x) && (76..=82).contains(&max_x),
        "link 2: the rect must land at the painter's global x (60..80), got \
         {min_x}..={max_x}"
    );
    assert!(
        (118..=122).contains(&min_y) && (136..=142).contains(&max_y),
        "link 2: the rect must land at the painter's global y (120..140), got \
         {min_y}..={max_y}"
    );

    // Link 3 — canvas_item_set_visible(ci, false) removes the pixels on the next
    // frame, driven from the script.
    ml.tree_mut()
        .call_script_method(painter, "hide_it", &[])
        .expect("hide_it is callable");
    let hidden = play_frame(ml.tree());
    assert!(
        pixels_of(&hidden, red()).is_empty(),
        "link 3: canvas_item_set_visible(ci, false) must remove the pixels on the \
         next frame"
    );

    // ...and it is genuinely a toggle, not a one-way delete.
    ml.tree_mut()
        .call_script_method(painter, "show_it", &[])
        .expect("show_it is callable");
    assert!(
        !pixels_of(&play_frame(ml.tree()), red()).is_empty(),
        "link 3: making the item visible again must bring the pixels back — \
         otherwise `hide` destroyed rather than hid"
    );

    // Link 4 — canvas_item_clear drops the commands while the item survives.
    ml.tree_mut()
        .call_script_method(painter, "clear_it", &[])
        .expect("clear_it is callable");
    assert!(
        pixels_of(&play_frame(ml.tree()), red()).is_empty(),
        "link 4: canvas_item_clear(ci) must remove the drawn commands"
    );
    assert!(
        gdscript_interop::canvas_server::canvas_item_exists(ci as u64),
        "link 4: clear removes the item's COMMANDS, not the item itself"
    );
}

/// An item parented to nothing must not draw, and one parented to a *hidden*
/// node must not either — the control that keeps link 2 honest, since a merge
/// that ignored the parent RID would draw the rect for every node in the scene.
#[test]
fn unparented_and_hidden_canvas_items_do_not_draw() {
    const ORPHAN_GD: &str = "\
extends Node2D
var ci = 0
func _ready():
    ci = RenderingServer.canvas_item_create()
    RenderingServer.canvas_item_add_rect(ci, Rect2(0, 0, 20, 20), Color(0, 1, 0, 1))
";
    gdobject::class_db::register_2d_classes();
    gdscript_interop::canvas_server::reset();

    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let painter = tree
        .get_node_or_null(scene_root, "Painter")
        .expect("Painter node");
    tree.attach_script(
        painter,
        Box::new(GDScriptNodeInstance::from_source(ORPHAN_GD, painter).unwrap()),
    );
    tree.process_script_ready(painter);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    let green = Color::new(0.0, 1.0, 0.0, 1.0);
    assert!(
        pixels_of(&play_frame(ml.tree()), green).is_empty(),
        "an item that was never parented to a canvas item must not draw"
    );

    // Now parent it, but hide the host node: Godot hides the canvas items
    // attached to a hidden node along with it.
    let ci = match script_var(&ml, painter, "ci") {
        Some(Variant::Int(i)) => i as u64,
        other => panic!("`ci` should be an RID Int, got {other:?}"),
    };
    let host = gdscript_interop::canvas_server::node_canvas_item(painter.raw());
    assert!(gdscript_interop::canvas_server::canvas_item_set_parent(
        ci, host
    ));
    assert!(
        !pixels_of(&play_frame(ml.tree()), green).is_empty(),
        "sanity: once parented to the visible painter, the item draws"
    );

    ml.tree_mut()
        .get_node_mut(painter)
        .unwrap()
        .set_property("visible", Variant::Bool(false));
    assert!(
        pixels_of(&play_frame(ml.tree()), green).is_empty(),
        "hiding the host node must hide the canvas items attached to it"
    );
}
