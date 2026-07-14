//! pat-nzgdu.22 — GPUParticles2D / CPUParticles2D are wired end-to-end from a
//! loaded scene through the frame loop into the 2D render bridge.
//!
//! Godot ground truth: a `GPUParticles2D`/`CPUParticles2D` node in a running
//! scene continuously emits and simulates particles and draws each live particle
//! every frame. Patina had a complete CPU `ParticleSimulator`, but nothing
//! connected a loaded scene → `MainLoop::step` → the 2D renderer: the simulator
//! never produced draw commands the render bridge could see.
//!
//! These tests prove the full wiring with a **scene-load-to-frame-output** path
//! (not module math):
//!   `.tscn` → `PackedScene::from_tscn` → `SceneTree` → `MainLoop::step` (which
//!   advances the simulator) → `scene_bridge::render_scene_to_frame` → pixels.
//! They assert (a) an emitting node with a nonzero initial velocity accumulates
//! live particles whose quads appear in the frame output at *moved* positions,
//! and (b) a `one_shot` emitter drains to zero and then produces no draws.

use gdcore::math::Color;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_bridge::render_scene_to_frame;
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdrender2d::SoftwareRenderer;

/// Load a `.tscn` string into a fresh `SceneTree`, returning the tree.
fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

/// A pixel is "lit" if any channel rises meaningfully above the black clear.
fn is_lit(c: Color) -> bool {
    c.r > 0.5 || c.g > 0.5 || c.b > 0.5
}

/// Render `ml`'s current tree and return (lit-pixel count, bottom-most lit y).
/// `bottom_y` is `None` when nothing is drawn.
fn render_stats(ml: &MainLoop, w: u32, h: u32) -> (usize, Option<usize>) {
    let mut renderer = SoftwareRenderer::new();
    let no_tex = |_p: &str| -> Option<gdcore::math::Vector2> { None };
    let frame = render_scene_to_frame(ml.tree(), &mut renderer, w, h, Color::BLACK, &no_tex);
    let width = frame.width as usize;
    let mut count = 0usize;
    let mut bottom_y: Option<usize> = None;
    for (i, px) in frame.pixels.iter().enumerate() {
        if is_lit(*px) {
            count += 1;
            let y = i / width;
            bottom_y = Some(bottom_y.map_or(y, |b| b.max(y)));
        }
    }
    (count, bottom_y)
}

/// A GPUParticles2D emitting straight down with a nonzero initial velocity must:
/// accumulate live particles when driven only by `MainLoop::step`, and have
/// those particles rasterized by the render bridge at positions that advance
/// *away* from the emitter origin over time.
#[test]
fn particle_2d_scene_renders_moving_particles() {
    // Emitter at (128, 40) firing straight down (direction +Y) at 120 px/s,
    // continuous emission (explosiveness 0), long lifetime so nothing dies mid
    // test. A 256×384 frame gives the cloud room to travel downward.
    let tscn = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Sparks" type="GPUParticles2D" parent="."]
position = Vector2(128, 40)
emitting = true
amount = 40
lifetime = 3.0
explosiveness = 0.0
spread = 0.0
direction = Vector2(0, 1)
initial_velocity_min = 120.0
initial_velocity_max = 120.0
"#;
    let tree = load(tscn);
    let pid = tree
        .get_node_by_path("/root/World/Sparks")
        .expect("Sparks particle node");

    let (w, h) = (256u32, 384u32);
    let mut ml = MainLoop::new(tree);

    // No live particles before stepping, and therefore nothing drawn.
    assert_eq!(ml.particle_count(pid), 0);
    let (count0, _) = render_stats(&ml, w, h);
    assert_eq!(count0, 0, "no particles should be drawn before stepping");

    // Early sample: a few frames in, some particles exist near the emitter.
    for _ in 0..15 {
        ml.step(1.0 / 60.0);
    }
    let early_live = ml.particle_count(pid);
    assert!(
        early_live > 0,
        "emitting node must accumulate live particles once stepped, got {early_live}"
    );
    let (early_count, early_bottom) = render_stats(&ml, w, h);
    assert!(
        early_count > 0,
        "live particles must be rasterized into the frame, got {early_count} lit px"
    );
    let early_bottom = early_bottom.expect("early frame has lit pixels");

    // Late sample: many frames later the oldest particles have travelled far
    // down the screen, so the drawn cloud's bottom edge has advanced.
    for _ in 0..75 {
        ml.step(1.0 / 60.0);
    }
    let late_live = ml.particle_count(pid);
    assert!(
        late_live > 0,
        "steady-state emitting node keeps live particles, got {late_live}"
    );
    let (late_count, late_bottom) = render_stats(&ml, w, h);
    assert!(late_count > 0, "late frame must still draw particles");
    let late_bottom = late_bottom.expect("late frame has lit pixels");

    // Movement proof #1: the cloud reaches well below the emitter origin (y=40).
    assert!(
        late_bottom > 120,
        "particles must render at moved positions well below the emitter (y=40); \
         bottom-most lit pixel at y={late_bottom}"
    );
    // Movement proof #2: the cloud's leading edge advanced between samples — the
    // particles are moving frame to frame, not sitting at the spawn point.
    assert!(
        late_bottom > early_bottom,
        "particle cloud must advance downward over time: early bottom y={early_bottom}, \
         late bottom y={late_bottom}"
    );
}

/// A one_shot GPUParticles2D fires a single burst, renders it, then drains to
/// zero live particles — after which the render bridge produces no particle
/// draws at all (the node stops drawing once its burst dies out).
#[test]
fn one_shot_particle_2d_drains_and_stops_drawing() {
    // explosiveness=1.0 → the whole `amount` fires as one burst after ~lifetime
    // seconds of accumulation, then one_shot latches emitting off.
    let tscn = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Burst" type="GPUParticles2D" parent="."]
position = Vector2(128, 60)
emitting = true
one_shot = true
amount = 12
lifetime = 0.5
explosiveness = 1.0
spread = 0.0
direction = Vector2(0, 1)
initial_velocity_min = 80.0
initial_velocity_max = 80.0
"#;
    let tree = load(tscn);
    let pid = tree
        .get_node_by_path("/root/World/Burst")
        .expect("Burst particle node");

    let (w, h) = (256u32, 256u32);
    let mut ml = MainLoop::new(tree);

    // Drive until the burst has fired and is visibly alive (~0.5s accumulation
    // to fire, then still within the 0.5s particle lifetime).
    let mut fired_live = 0usize;
    let mut fired_drawn = 0usize;
    for _ in 0..40 {
        ml.step(1.0 / 60.0);
        let live = ml.particle_count(pid);
        if live > 0 {
            fired_live = live;
            fired_drawn = render_stats(&ml, w, h).0;
        }
    }
    assert!(
        fired_live > 0,
        "one_shot emitter must fire a burst of live particles"
    );
    assert!(
        fired_drawn > 0,
        "the fired burst must be rasterized into the frame, got {fired_drawn} lit px"
    );

    // Keep stepping well past the particle lifetime: the burst dies out and the
    // one_shot emitter does not refill.
    for _ in 0..90 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        ml.particle_count(pid),
        0,
        "one_shot burst must drain to zero live particles"
    );
    let (drained_count, _) = render_stats(&ml, w, h);
    assert_eq!(
        drained_count, 0,
        "a drained one_shot emitter must produce no particle draws, got {drained_count} lit px"
    );
}
