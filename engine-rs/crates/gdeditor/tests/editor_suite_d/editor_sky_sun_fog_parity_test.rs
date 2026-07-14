//! App-level Godot-parity tests for the editor viewport sky: the sun disc and
//! the horizon haze ("fog") band.
//!
//! Regression for the two parity bugs that made Patina's viewport sky a flat
//! two-tone gradient while Godot shows a sun and a bright hazy horizon:
//!
//! 1. The procedural sky never rendered directional-light sun discs — Godot's
//!    `ProceduralSkyMaterial` shader (scene/resources/3d/sky_material.cpp)
//!    draws up to 4 `LIGHTn_*` suns with a `pow(1 - c2, inv_sun_curve)` halo.
//! 2. The gradient used `pow(|y|, curve)` instead of Godot's
//!    `mix(top, horizon, pow(1 ∓ y, 0.6 / curve))`, which collapsed the
//!    whitish horizon band (the "fog" look) into a hard color step.
//!
//! These tests drive the real editor render path (`render_editor_3d_frame`,
//! the reference the GPU path must match) over a real scene tree.

use gdcore::math::Color;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::render_editor_3d_frame;
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::Node;
use gdscene::SceneTree;

const W: u32 = 200;
const H: u32 = 150;

/// A scene whose only light is a default-orientation DirectionalLight3D
/// (like fixtures/scenes/outdoor_3d.tscn's "Sun"): it shines along its local
/// −Z, so the sun sits on the horizon toward +Z.
fn scene_with_sun() -> SceneTree {
    gdobject::class_db::register_3d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();
    tree
}

fn empty_scene() -> SceneTree {
    gdobject::class_db::register_3d_classes();
    SceneTree::new()
}

/// The brightest "white-ness" in the frame: max over pixels of the dimmest
/// channel. The sky gradient never exceeds ~0.67 on its dimmest channel, so
/// anything above 0.9 is the (white) sun disc.
fn max_min_channel(fb: &FrameBuffer) -> f32 {
    fb.pixels
        .iter()
        .map(|c: &Color| c.r.min(c.g).min(c.b))
        .fold(0.0f32, f32::max)
}

/// A scene DirectionalLight3D must put a sun disc in the viewport sky in the
/// direction the light comes from — and only there (Godot draws the sun from
/// the scene's directional lights; there is no sun looking the other way).
#[test]
fn scene_directional_light_draws_sun_disc_in_sky() {
    let tree = scene_with_sun();

    // Face +Z (yaw π): the sun direction for an identity-basis light.
    let mut vp = Viewport3D::new(W, H);
    vp.camera.yaw = std::f32::consts::PI;
    vp.camera.pitch = 0.0;
    let frame = render_editor_3d_frame(&vp, &tree);
    assert!(
        max_min_channel(&frame) > 0.85,
        "sun disc must be visible facing the light, brightest={:.3}",
        max_min_channel(&frame)
    );

    // Face −Z: no sun there.
    let mut vp_away = Viewport3D::new(W, H);
    vp_away.camera.yaw = 0.0;
    vp_away.camera.pitch = 0.0;
    let frame_away = render_editor_3d_frame(&vp_away, &tree);
    assert!(
        max_min_channel(&frame_away) < 0.8,
        "no sun disc opposite the light, brightest={:.3}",
        max_min_channel(&frame_away)
    );
}

/// With no scene light, the preview sun (Godot's default: altitude 60°,
/// azimuth 150°) draws the disc; toggling the preview sun off removes it,
/// exactly like Godot removing the preview DirectionalLight3D.
#[test]
fn preview_sun_draws_disc_and_toggle_removes_it() {
    let tree = empty_scene();

    // Look straight at the preview sun: direction toward it is
    // (0.25, 0.866, -0.433) → pitch 60° up, yaw -30°.
    let mut vp = Viewport3D::new(W, H);
    vp.camera.pitch = 60f32.to_radians();
    vp.camera.yaw = -30f32.to_radians();
    let frame = render_editor_3d_frame(&vp, &tree);
    assert!(
        max_min_channel(&frame) > 0.85,
        "preview sun disc must be visible, brightest={:.3}",
        max_min_channel(&frame)
    );

    let mut vp_off = Viewport3D::new(W, H);
    vp_off.camera.pitch = 60f32.to_radians();
    vp_off.camera.yaw = -30f32.to_radians();
    vp_off.environment.toggle_sun();
    let frame_off = render_editor_3d_frame(&vp_off, &tree);
    assert!(
        max_min_channel(&frame_off) < 0.8,
        "disabling the preview sun must remove the disc, brightest={:.3}",
        max_min_channel(&frame_off)
    );
}

/// The horizon must show Godot's bright haze band (the "fog" look) on both
/// sides of the horizon line: whitish just above and just below, darker at
/// the zenith. The old `pow(|y|, curve)` gradient failed all three.
#[test]
fn sky_horizon_haze_matches_godot() {
    let tree = empty_scene();

    // Level camera: the horizon sits at the vertical center of the frame.
    // The preview sun is up and behind-left, far from these samples.
    let mut vp = Viewport3D::new(W, H);
    vp.camera.pitch = 0.0;
    vp.camera.yaw = 0.0;
    let frame = render_editor_3d_frame(&vp, &tree);

    // Sample away from the center column so the origin-axis overlay lines
    // can't touch the pixels.
    let x = (3 * W / 4) as usize;
    let w = W as usize;
    let above = frame.pixels[(H as usize / 2 - 3) * w + x];
    let below = frame.pixels[(H as usize / 2 + 2) * w + x];
    let top = frame.pixels[1 * w + x];

    // These are the final display pixels — the editor preview applies filmic
    // tonemapping (which lifts midtones), so the meaningful parity check is the
    // *relative* horizon-brighter-than-zenith relationship, not raw cutoffs.
    //
    // Just above the horizon: pow(1 - y, 4) stays near 1 → whitish horizon
    // color, not the darker top color the old `pow(|y|, curve)` produced.
    assert!(
        above.r > 0.55,
        "sky just above the horizon must be hazy-bright, got r={:.3}",
        above.r
    );
    // Zenith-ward: the gradient must fall back toward the darker top color
    // (below the horizon band even after the filmic midtone lift).
    assert!(
        top.r < 0.62,
        "sky far above the horizon must darken toward the top color, got r={:.3}",
        top.r
    );
    assert!(
        above.r > top.r + 0.08,
        "horizon haze must be brighter than the upper sky: horizon r={:.3}, top r={:.3}",
        above.r,
        top.r
    );
    // Just below the horizon: pow(1 + y, 30) ≈ 0.5+ → a bright band above
    // the dark ground color, Godot's below-horizon haze.
    assert!(
        below.r > 0.4,
        "ground just below the horizon must show the haze band, got r={:.3}",
        below.r
    );
}
