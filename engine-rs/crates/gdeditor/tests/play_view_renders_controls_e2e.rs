//! pat-0q2kx.19 — the Play view rasterizes real themed Controls.
//!
//! Godot draws every Control each frame from its `NOTIFICATION_DRAW` handler with
//! the resolved theme, after `Container::_sort_children` has laid the children out
//! (upstream/godot/scene/gui/*.cpp, container.cpp). Patina had both halves —
//! `control_render::render_control_tree` and `control::sort_container_tree` — but
//! the Play frame drew only Sprite2D/Polygon2D/Line2D/ColorRect/Label and rendered
//! every other control as an empty `Render2DKind::Marker`, so a themed UI appeared
//! as markers piled at the origin.
//!
//! This asserts the pixels, through the same two public functions the Play branch
//! of `examples/editor.rs` now calls: `build_control_render_items` (containers
//! sorted, controls drawn, primitives mapped) then `rasterize_render_list`.

use gdcore::math::Vector2;
use gdeditor::viewport_2d_render::{
    build_control_render_items, rasterize_render_list, CONTROL_Z_BASE, RUNTIME_CLEAR_COLOR,
};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

const W: u32 = 200;
const H: u32 = 160;

/// `Panel > VBoxContainer > [Button("OK"), ProgressBar(value=50)]`, the scene the
/// bead names, sized so the container has room to stack its children.
fn ui_scene() -> (SceneTree, gdscene::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let panel = tree.add_child(root, Node::new("Panel", "Panel")).unwrap();
    if let Some(n) = tree.get_node_mut(panel) {
        n.set_property("size", Variant::Vector2(Vector2::new(W as f32, H as f32)));
    }

    let vbox = tree
        .add_child(panel, Node::new("VBoxContainer", "VBoxContainer"))
        .unwrap();
    if let Some(n) = tree.get_node_mut(vbox) {
        n.set_property("size", Variant::Vector2(Vector2::new(W as f32, H as f32)));
    }

    // `custom_minimum_size` is what a container distributes space from
    // (`control::effective_min` = max(custom_minimum_size, min_size)). Godot
    // derives an implicit minimum from the theme font and content margins, which
    // Patina does not compute yet, so an authored minimum is what makes a VBox
    // stack rather than give every child zero height — exactly what a `.tscn`
    // author sets on a button row.
    let button = tree
        .add_child(vbox, Node::new("OkButton", "Button"))
        .unwrap();
    if let Some(n) = tree.get_node_mut(button) {
        n.set_property("text", Variant::String("OK".into()));
        n.set_property(
            "custom_minimum_size",
            Variant::Vector2(Vector2::new(W as f32, 40.0)),
        );
    }

    let bar = tree
        .add_child(vbox, Node::new("Bar", "ProgressBar"))
        .unwrap();
    if let Some(n) = tree.get_node_mut(bar) {
        n.set_property("value", Variant::Float(50.0));
        n.set_property("max_value", Variant::Float(100.0));
        n.set_property(
            "custom_minimum_size",
            Variant::Vector2(Vector2::new(W as f32, 24.0)),
        );
    }

    (tree, root)
}

fn non_background_pixels(fb: &gdrender2d::renderer::FrameBuffer) -> usize {
    let bg = RUNTIME_CLEAR_COLOR;
    let mut n = 0;
    for y in 0..H {
        for x in 0..W {
            let p = fb.get_pixel(x, y);
            if (p.r - bg.r).abs() > 0.01 || (p.g - bg.g).abs() > 0.01 || (p.b - bg.b).abs() > 0.01 {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn play_view_renders_controls_e2e() {
    let (mut tree, root) = ui_scene();

    let items = build_control_render_items(&mut tree, root, Vector2::new(W as f32, H as f32));

    // 1. The controls produced real drawing, not markers.
    assert!(
        !items.is_empty(),
        "the Control subtree produced no render items at all"
    );
    assert!(
        items.iter().all(|i| !matches!(
            i.kind,
            gdeditor::viewport_2d_render::Render2DKind::Marker { .. }
        )),
        "controls are still falling through to Marker: {:?}",
        items.iter().map(|i| &i.kind).collect::<Vec<_>>()
    );

    // 2. UI paints above plain 2D nodes.
    assert!(
        items.iter().all(|i| i.z_index >= CONTROL_Z_BASE),
        "control items must carry the CONTROL_Z_BASE paint-order floor"
    );

    // 3. The container stacked its children instead of leaving them at the origin.
    //    Collect the distinct y origins of the filled rects — a VBox must produce
    //    at least two different ones (button row, progress row).
    let mut ys: Vec<i32> = items
        .iter()
        .filter(|i| {
            matches!(
                i.kind,
                gdeditor::viewport_2d_render::Render2DKind::ColorRect { .. }
            )
        })
        .map(|i| i.global_transform.origin.y.round() as i32)
        .collect();
    ys.sort_unstable();
    ys.dedup();
    assert!(
        ys.len() >= 2,
        "VBoxContainer did not stack its children — all filled rects share y {ys:?}"
    );

    // 4. The pixels actually land in the frame.
    let fb = rasterize_render_list(&items, W, H, RUNTIME_CLEAR_COLOR, None);
    let painted = non_background_pixels(&fb);
    assert!(
        painted > 200,
        "expected the button stylebox and progress fill to cover real area, got {painted} px"
    );

    // 5. The ProgressBar is half filled: its fill rect spans about half the bar's
    //    width, so *some* ColorRect is between 30% and 70% of the container width.
    let half_ish = items.iter().any(|i| {
        if let gdeditor::viewport_2d_render::Render2DKind::ColorRect { size, .. } = &i.kind {
            let frac = size.x / W as f32;
            (0.3..0.7).contains(&frac)
        } else {
            false
        }
    });
    assert!(
        half_ish,
        "no rect matches a 50% ProgressBar fill; rect widths were {:?}",
        items
            .iter()
            .filter_map(|i| match &i.kind {
                gdeditor::viewport_2d_render::Render2DKind::ColorRect { size, .. } => Some(size.x),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
}

/// Layout must be idempotent: rendering the same tree twice cannot drift the
/// container's arrangement, or the Play view would creep frame after frame.
#[test]
fn repeated_frames_produce_stable_layout() {
    let (mut tree, root) = ui_scene();
    let size = Vector2::new(W as f32, H as f32);

    let first = build_control_render_items(&mut tree, root, size);
    let second = build_control_render_items(&mut tree, root, size);

    let origins = |items: &[gdeditor::viewport_2d_render::Render2DItem]| -> Vec<(i32, i32)> {
        items
            .iter()
            .map(|i| {
                (
                    i.global_transform.origin.x.round() as i32,
                    i.global_transform.origin.y.round() as i32,
                )
            })
            .collect()
    };
    assert_eq!(
        origins(&first),
        origins(&second),
        "container layout drifted between frames"
    );
}
