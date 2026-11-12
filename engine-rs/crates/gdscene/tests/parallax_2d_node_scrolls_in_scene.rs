//! pat-s3rrv.86 item (2) — the standalone `Parallax2D` node scrolls.
//!
//! The class was registered in ClassDB with all ten of its documented
//! properties and **no behaviour at all**: a `.tscn` containing one loaded, kept
//! every property, and never moved a pixel. Registered-but-inert, the exact
//! failure this audit keeps turning up.
//!
//! ⚠️ **`Parallax2D` is a `Node2D`, not a `CanvasLayer`.** The camera transform
//! DOES apply to it, so it scrolls by being placed in the WORLD — and
//! `scroll_scale`'s endpoints are therefore the OPPOSITE of
//! `ParallaxLayer::motion_scale`:
//!
//! * `scroll_scale = 1` ⇒ a fixed world position, i.e. moves with the world at
//!   the full camera rate;
//! * `scroll_scale = 0` ⇒ tracks the view exactly, i.e. pinned to the screen.
//!
//! On a ParallaxLayer those two are the other way round. All three rates are
//! pinned below rather than a single midpoint, because a midpoint alone cannot
//! tell the two conventions apart.

use gdcore::math::Vector2;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// The three `scroll_scale` endpoints side by side, plus a frozen one.
const WORLD: &str = r#"[gd_scene format=3]

[node name="Main" type="Node"]

[node name="Cam" type="Camera2D" parent="."]
position = Vector2(0, 0)
current = true

[node name="World" type="Parallax2D" parent="."]
scroll_scale = Vector2(1, 1)

[node name="Half" type="Parallax2D" parent="."]
scroll_scale = Vector2(0.5, 0.5)

[node name="Screen" type="Parallax2D" parent="."]
scroll_scale = Vector2(0, 0)

[node name="Frozen" type="Parallax2D" parent="."]
scroll_scale = Vector2(0.5, 0.5)
ignore_camera_scroll = true
"#;

fn load() -> MainLoop {
    let scene = PackedScene::from_tscn(WORLD).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    MainLoop::new(tree)
}

fn node_pos(ml: &MainLoop, name: &str) -> Vector2 {
    let path = format!("/root/Main/{name}");
    match ml
        .tree()
        .get_node_by_path(&path)
        .and_then(|id| ml.tree().get_node(id))
        .map(|n| n.get_property("position"))
    {
        Some(Variant::Vector2(v)) => v,
        other => panic!("{path} position should be a Vector2, got {other:?}"),
    }
}

fn move_camera_to(ml: &mut MainLoop, x: f32) {
    let cam = ml
        .tree()
        .get_node_by_path("/root/Main/Cam")
        .expect("the camera is in the scene");
    if let Some(node) = ml.tree_mut().get_node_mut(cam) {
        node.set_property("position", Variant::Vector2(Vector2::new(x, 0.0)));
    }
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }
}

/// Travel of each named node across a 1000px camera move.
fn travels(ml: &mut MainLoop, names: &[&str]) -> Vec<f32> {
    move_camera_to(ml, 0.0);
    let start: Vec<f32> = names.iter().map(|n| node_pos(ml, n).x).collect();
    move_camera_to(ml, 1000.0);
    names
        .iter()
        .enumerate()
        .map(|(i, n)| node_pos(ml, n).x - start[i])
        .collect()
}

/// **The acceptance test.** All three `scroll_scale` endpoints, in one pass.
///
/// The three travels must be 0, 500 and 1000 for a 1000px camera move. A node
/// that never moved would give 0 for all three and satisfy only the first — so
/// the inert case this bead is about fails on the other two.
#[test]
fn parallax_2d_scrolls_at_its_configured_rate() {
    let mut ml = load();
    let got = travels(&mut ml, &["World", "Half", "Screen"]);

    // ⚠️ scroll_scale 1 holds a FIXED WORLD POSITION. Reading it as the
    // ParallaxLayer convention would make this the moving one and `Screen` the
    // still one — the whole node inverted, with a midpoint test unable to see it.
    assert!(
        got[0].abs() < 1e-2,
        "scroll_scale 1 must hold a fixed world position, travelled {}",
        got[0]
    );
    assert!(
        (got[1] - 500.0).abs() < 1e-2,
        "scroll_scale 0.5 must travel half the camera's 1000px, got {}",
        got[1]
    );
    assert!(
        (got[2] - 1000.0).abs() < 1e-2,
        "scroll_scale 0 must track the view exactly, got {}",
        got[2]
    );
}

/// The rates are ordered and distinct — the cheap check that survives any
/// rescaling of the fixture.
///
/// Present because all three assertions above could be satisfied by a formula
/// that happened to be right at these numbers while collapsing the endpoints
/// together elsewhere.
#[test]
fn parallax_2d_rates_are_distinct_and_ordered() {
    let mut ml = load();
    let got = travels(&mut ml, &["World", "Half", "Screen"]);

    assert!(
        got[0] < got[1] && got[1] < got[2],
        "a smaller scroll_scale must scroll less: {got:?}"
    );
}

/// `ignore_camera_scroll` freezes the node against camera movement.
///
/// It shares `scroll_scale` with `Half`, so the two differ in exactly one
/// property and the frozen one must not move while its twin does.
#[test]
fn parallax_2d_ignore_camera_scroll_holds_the_node_still() {
    let mut ml = load();
    let got = travels(&mut ml, &["Frozen", "Half"]);

    assert!(
        got[0].abs() < 1e-2,
        "ignore_camera_scroll must freeze the node, travelled {}",
        got[0]
    );
    assert!(
        (got[1] - 500.0).abs() < 1e-2,
        "its twin without the flag must still scroll, got {}",
        got[1]
    );
}
