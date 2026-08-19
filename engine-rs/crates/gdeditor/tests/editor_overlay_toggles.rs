//! pat-s3rrv.82 item (1b) — the editor viewport's gizmos can be switched off.
//!
//! `OverlaySettings` / `CanvasOverlay` already existed and the navmesh overlay
//! was gated on them, but the editor's own scene renderer drew collision shapes
//! and navigation regions **unconditionally**, so there was no state a View-menu
//! item could drive.
//!
//! ⚠️ The gate is opt-in. `OverlaySettings::default()` enables only the origin
//! cross and the lock/group icons, so routing the existing entry points through
//! it would have turned these gizmos OFF for every current caller — a silent
//! change to what the editor draws, shipped as part of adding a toggle. The
//! pre-existing `render_scene*` functions therefore still draw everything, and
//! only `render_scene_with_overlays` honours the set.

use gdcore::math::{Color, Vector2};
use gdeditor::scene_renderer::{render_scene, render_scene_with_overlays};
use gdeditor::viewport_2d::{CanvasOverlay, OverlaySettings};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const W: u32 = 200;
const H: u32 = 200;
/// Matches `scene_renderer::BG_COLOR`, which is private.
const BG: Color = Color::new(0.08, 0.08, 0.1, 1.0);

/// `Root -> CollisionShape2D`, optionally omitting the shape entirely.
fn scene(with_shape: bool) -> SceneTree {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();
    if with_shape {
        let mut cs = Node::new("Col", "CollisionShape2D");
        cs.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        cs.set_property("size", Variant::Vector2(Vector2::new(40.0, 40.0)));
        tree.add_child(root, cs).unwrap();
    }
    tree
}

/// Counts pixels that differ from the background.
fn drawn(fb: &gdrender2d::renderer::FrameBuffer) -> usize {
    let mut n = 0;
    for y in 0..fb.height {
        for x in 0..fb.width {
            if fb.get_pixel(x, y) != BG {
                n += 1;
            }
        }
    }
    n
}

fn overlays_with(collision: bool) -> OverlaySettings {
    let mut o = OverlaySettings::new();
    if collision {
        o.enable(CanvasOverlay::CollisionShapes);
    } else {
        o.disable(CanvasOverlay::CollisionShapes);
    }
    o
}

/// ⚠️ The no-regression control: the pre-existing entry point is unchanged.
///
/// Every current caller and test renders through `render_scene`, and the gizmo
/// must still appear there — otherwise adding a toggle silently changed what the
/// editor draws for everyone who never asked for one.
#[test]
fn the_default_entry_point_still_draws_collision_shapes() {
    let with = drawn(&render_scene(&scene(true), None, W, H));
    let without = drawn(&render_scene(&scene(false), None, W, H));
    assert!(
        with > without,
        "render_scene must still draw a CollisionShape2D gizmo unconditionally: \
         {with} lit pixels with the node vs {without} without it"
    );
}

/// The overlay set gates the gizmo.
#[test]
fn disabling_the_collision_overlay_hides_the_gizmo() {
    let on = drawn(&render_scene_with_overlays(
        &scene(true),
        None,
        W,
        H,
        1.0,
        (0.0, 0.0),
        &overlays_with(true),
    ));
    let off = drawn(&render_scene_with_overlays(
        &scene(true),
        None,
        W,
        H,
        1.0,
        (0.0, 0.0),
        &overlays_with(false),
    ));
    assert!(
        on > off,
        "enabling CanvasOverlay::CollisionShapes must draw more than disabling \
         it; got {on} lit pixels on vs {off} off"
    );
}

/// 🔑 Switching the overlay off must draw **nothing**, not a different mark.
///
/// ⚠️ This is the assertion that catches the tempting implementation. Gating the
/// arm with a match GUARD (`"CollisionShape2D" if shows(..) =>`) makes a
/// disabled gizmo fall through to the renderer's `_ =>` catch-all, which draws a
/// small default dot — so the node would still be marked, just differently.
/// Comparing against a scene with the node REMOVED is what distinguishes
/// "hidden" from "drawn as something else"; a mere `on > off` count cannot.
#[test]
fn a_disabled_overlay_draws_nothing_rather_than_a_fallback_mark() {
    let hidden = drawn(&render_scene_with_overlays(
        &scene(true),
        None,
        W,
        H,
        1.0,
        (0.0, 0.0),
        &overlays_with(false),
    ));
    let absent = drawn(&render_scene_with_overlays(
        &scene(false),
        None,
        W,
        H,
        1.0,
        (0.0, 0.0),
        &overlays_with(false),
    ));
    assert_eq!(
        hidden, absent,
        "a CollisionShape2D with its overlay off must render identically to no \
         CollisionShape2D at all ({hidden} lit vs {absent}); a small difference \
         means it fell through to the default-dot arm instead of being skipped"
    );
}

/// An absent overlay set means "draw everything", which is NOT the same as an
/// empty set — the distinction that keeps the pre-existing callers working.
#[test]
fn an_empty_overlay_set_is_not_the_same_as_no_overlay_set() {
    let empty = OverlaySettings::new();
    assert!(
        !empty.is_enabled(CanvasOverlay::CollisionShapes),
        "the default set must NOT enable collision shapes — Godot enables only \
         the origin cross and lock/group icons"
    );
    let gated = drawn(&render_scene_with_overlays(
        &scene(true),
        None,
        W,
        H,
        1.0,
        (0.0, 0.0),
        &empty,
    ));
    let ungated = drawn(&render_scene(&scene(true), None, W, H));
    assert!(
        ungated > gated,
        "passing the default set must hide the gizmo while the no-set entry \
         point still draws it: {ungated} lit ungated vs {gated} gated"
    );
}
