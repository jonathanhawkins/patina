//! pat-0q2kx.51 — live particles reach the editor's Play canvas and edit
//! viewport, and `local_coords` decides whether they follow the emitter.
//!
//! Godot draws every live particle each frame, textured by the emitter's
//! `texture` and — with `local_coords = false`, the default — positioned in
//! GLOBAL space, so moving the emitter leaves the existing cloud behind
//! (`scene/2d/gpu_particles_2d.cpp`, `doc/classes/GPUParticles2D.xml`).
//!
//! Patina simulated particles correctly and `gdscene::scene_bridge` could
//! rasterize them, but no production surface called it: neither
//! `viewport_2d_render::build_runtime_render_list` (the Play canvas) nor
//! `scene_renderer` (the edit viewport) had a particle arm, so particles were
//! invisible in the product while the gdscene tests passed. `local_coords` was
//! read into the emitter config and then never used at all.
//!
//! These drive the real render entry points on a `.tscn`-loaded scene stepped by
//! the real `MainLoop`, and assert on produced pixels.

use gdcore::math::{Color, Vector2};
use gdeditor::scene_renderer;
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::{
    build_runtime_render_list, rasterize_render_list, Render2DItem, Render2DKind,
    RUNTIME_CLEAR_COLOR,
};
use gdrender2d::renderer::FrameBuffer;
use gdrender2d::texture::Texture2D;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::{MainLoop, NodeId, SceneTree};
use gdvariant::Variant;

const W: u32 = 320;
const H: u32 = 240;

/// An emitter firing straight along +X at a fixed 80 px/s, so a particle's
/// travel is predictable rather than a random spray: `spread = 0` removes the
/// angular jitter and min == max removes the speed jitter.
///
/// Godot keeps these on a `ParticleProcessMaterial`; Patina reads the same keys
/// off the node for both particle classes (see `MainLoop::process_particles`),
/// which is what a `.tscn` can express here.
fn scene_source(local_coords: bool) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Fx" type="GPUParticles2D" parent="."]
position = Vector2(60, 120)
amount = 16
lifetime = 4.0
emitting = true
local_coords = {local_coords}
spread = 0.0
direction = Vector2(1, 0)
initial_velocity_min = 80.0
initial_velocity_max = 80.0
"#
    )
}

/// Loads the scene through the real `.tscn` path and returns the running loop
/// plus the emitter's id.
fn running_scene(local_coords: bool) -> (MainLoop, NodeId) {
    // The render list admits a node only if `is_class("CanvasItem")`, which
    // resolves through the ClassDB — and the ClassDB starts EMPTY. Without this
    // the emitter never enters the list and every assertion would be vacuous.
    gdobject::class_db::register_2d_classes();

    let scene = PackedScene::from_tscn(&scene_source(local_coords)).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let emitter = tree
        .get_node_or_null(scene_root, "Fx")
        .expect("Fx emitter node");
    (MainLoop::new(tree), emitter)
}

/// Advances the loop by `seconds` in 60 Hz frames, exactly as the editor's Play
/// loop does.
fn run_for(loop_: &mut MainLoop, seconds: f64) {
    let frames = (seconds * 60.0).round() as u32;
    for _ in 0..frames {
        loop_.step(1.0 / 60.0);
    }
}

/// The Play canvas frame: the two production calls the editor's Play tab makes.
fn play_frame(tree: &SceneTree, cache: Option<&mut TextureCache>) -> FrameBuffer {
    let items = build_runtime_render_list(tree);
    rasterize_render_list(&items, W, H, RUNTIME_CLEAR_COLOR, cache)
}

/// Coordinates of every pixel that differs from the clear color — the pixels the
/// scene itself drew.
fn lit_pixels(fb: &FrameBuffer) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for y in 0..fb.height {
        for x in 0..fb.width {
            let p = fb.pixels[(y * fb.width + x) as usize];
            if !same_color(p, RUNTIME_CLEAR_COLOR) {
                out.push((x, y));
            }
        }
    }
    out
}

/// How many pixels two same-sized frames disagree on — a diff summary that keeps
/// a failure message short instead of dumping two full framebuffers.
fn differing_pixels(a: &FrameBuffer, b: &FrameBuffer) -> usize {
    assert_eq!(
        (a.width, a.height),
        (b.width, b.height),
        "frames must be the same size to diff"
    );
    a.pixels
        .iter()
        .zip(b.pixels.iter())
        .filter(|(x, y)| !same_color(**x, **y))
        .count()
}

fn same_color(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 0.01
        && (a.g - b.g).abs() < 0.01
        && (a.b - b.b).abs() < 0.01
        && (a.a - b.a).abs() < 0.01
}

/// Mean X of a set of pixels — a cloud-position summary that does not depend on
/// how many particles happen to be alive.
fn mean_x(pixels: &[(u32, u32)]) -> f32 {
    assert!(!pixels.is_empty(), "no lit pixels to average");
    pixels.iter().map(|(x, _)| *x as f32).sum::<f32>() / pixels.len() as f32
}

/// The emitter's particle item from the runtime render list.
fn particle_item(items: &[Render2DItem], emitter: NodeId) -> &Render2DItem {
    items
        .iter()
        .find(|i| i.node_id == emitter.raw())
        .expect("the emitter must be in the runtime render list")
}

/// Every live particle's position in GLOBAL canvas space: the published
/// emitter-local point put through the item transform the rasterizer draws with.
fn global_particle_positions(tree: &SceneTree, emitter: NodeId) -> Vec<Vector2> {
    let items = build_runtime_render_list(tree);
    let item = particle_item(&items, emitter);
    match &item.kind {
        Render2DKind::Particles { particles, .. } => particles
            .iter()
            .map(|q| item.global_transform.xform(q.position))
            .collect(),
        other => panic!("expected a Particles item for the emitter, got {other:?}"),
    }
}

fn set_prop(tree: &mut SceneTree, id: NodeId, key: &str, value: Variant) {
    tree.get_node_mut(id)
        .expect("node still in tree")
        .set_property(key, value);
}

/// **The acceptance test.** A `.tscn` emitter produces moving particle pixels in
/// the Play canvas *and* in the edit viewport.
#[test]
fn particles_render_in_editor_play_canvas() {
    let (mut running, emitter) = running_scene(false);

    // Nothing has been stepped yet: no live particles, so nothing to draw.
    let before = play_frame(running.tree(), None);
    assert!(
        lit_pixels(&before).is_empty(),
        "an unstepped scene has no live particles, so the Play canvas must be empty"
    );

    // -- Play canvas: particles are visible, and they move ---------------------
    run_for(&mut running, 0.5);
    let first = play_frame(running.tree(), None);
    let first_lit = lit_pixels(&first);
    assert!(
        !first_lit.is_empty(),
        "an emitting GPUParticles2D must draw pixels in the Play canvas — the \
         particle arm is missing from build_runtime_render_list/rasterize_render_list"
    );

    run_for(&mut running, 0.5);
    let second = play_frame(running.tree(), None);
    let second_lit = lit_pixels(&second);
    assert!(
        !second_lit.is_empty(),
        "particles must still be drawn after a second of simulation"
    );
    assert_ne!(
        first_lit, second_lit,
        "particle pixels must move frame-to-frame — an identical set means a \
         static (or stale) draw rather than the live simulation"
    );
    // Travelling at +80 px/s along +X, the cloud's centre must advance.
    assert!(
        mean_x(&second_lit) > mean_x(&first_lit) + 5.0,
        "the cloud must travel along its emission direction: mean x went from {} to {}",
        mean_x(&first_lit),
        mean_x(&second_lit)
    );

    // -- Edit viewport: the same live particles reach the editor preview -------
    //
    // The editor frame carries chrome (grid, rulers, labels) that is identical
    // across these three renders, so every pixel difference is particles.
    let (idle, _) = running_scene(false);
    let idle_frame = scene_renderer::render_scene(idle.tree(), None, W, H);
    let edit_frame = scene_renderer::render_scene(running.tree(), None, W, H);
    assert!(
        differing_pixels(&idle_frame, &edit_frame) > 0,
        "live particles must be drawn in the edit viewport, not only in Play — \
         the stepped scene renders pixel-identically to the unstepped one"
    );

    run_for(&mut running, 0.5);
    let edit_later = scene_renderer::render_scene(running.tree(), None, W, H);
    assert!(
        differing_pixels(&edit_frame, &edit_later) > 0,
        "edit-viewport particles must move frame-to-frame like the Play canvas"
    );
}

/// With `local_coords = false` (Godot's default) already-spawned particles hold
/// their global position when the emitter moves; with `true` they follow it.
#[test]
fn particles_honor_local_coords_when_emitter_moves() {
    const SHIFT: f32 = 200.0;

    for (local_coords, follows) in [(false, false), (true, true)] {
        let (mut running, emitter) = running_scene(local_coords);
        run_for(&mut running, 0.5);

        // Freeze emission so the comparison is over the same particles rather
        // than a cloud refilled at the emitter's new position.
        set_prop(
            running.tree_mut(),
            emitter,
            "emitting",
            Variant::Bool(false),
        );
        let before = global_particle_positions(running.tree(), emitter);
        assert!(
            !before.is_empty(),
            "the emitter must have live particles to compare (local_coords={local_coords})"
        );

        // Move the emitter, then step just enough to republish.
        set_prop(
            running.tree_mut(),
            emitter,
            "position",
            Variant::Vector2(Vector2::new(60.0 + SHIFT, 120.0)),
        );
        running.step(1.0 / 600.0);

        let after = global_particle_positions(running.tree(), emitter);
        assert_eq!(
            before.len(),
            after.len(),
            "no particle should be born or die across a 1/600s step \
             (local_coords={local_coords})"
        );

        let moved: f32 = (mean(&after) - mean(&before)).x;
        if follows {
            assert!(
                (moved - SHIFT).abs() < 1.0,
                "local_coords = true must carry the cloud with the emitter: \
                 expected ~{SHIFT}px, got {moved}px"
            );
        } else {
            assert!(
                moved.abs() < 1.0,
                "local_coords = false leaves spawned particles in global space: \
                 moving the emitter {SHIFT}px must not move them, got {moved}px"
            );
        }
    }
}

/// The mean of a set of points.
fn mean(points: &[Vector2]) -> Vector2 {
    assert!(!points.is_empty(), "no points to average");
    let n = points.len() as f32;
    let sum = points.iter().fold(Vector2::ZERO, |acc, p| {
        Vector2::new(acc.x + p.x, acc.y + p.y)
    });
    Vector2::new(sum.x / n, sum.y / n)
}

/// A particle quad samples the emitter's `texture` when one is set.
#[test]
fn particles_draw_the_emitter_texture() {
    let (mut running, emitter) = running_scene(false);
    set_prop(
        running.tree_mut(),
        emitter,
        "texture",
        Variant::String("res://fx.png".into()),
    );
    run_for(&mut running, 0.5);

    // A distinctive solid texture: nothing else in this scene is magenta, and the
    // untextured fallback quad is the particle's own (white) color.
    const MAGENTA: Color = Color::new(1.0, 0.0, 1.0, 1.0);
    let mut cache = TextureCache::new(".");
    cache.insert("res://fx.png", Texture2D::solid(8, 8, MAGENTA));

    let frame = play_frame(running.tree(), Some(&mut cache));
    let magenta = frame
        .pixels
        .iter()
        .filter(|p| same_color(**p, MAGENTA))
        .count();
    assert!(
        magenta > 0,
        "particles must be drawn with the emitter's texture — no textured pixels found"
    );

    // The 8×8 texture covers more area than the 4×4 fallback quad, so a textured
    // draw is also strictly larger than the untextured one.
    let untextured = play_frame(running.tree(), None);
    assert!(
        magenta > lit_pixels(&untextured).len() / 2,
        "the textured draw must cover at least as much as the fallback quad: \
         {magenta} textured pixels vs {} fallback",
        lit_pixels(&untextured).len()
    );
}
