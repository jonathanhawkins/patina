//! pat-s3rrv.251 acceptance: a `CompositorEffect` callback runs mid-frame and
//! its mutation reaches the output.
//!
//! # The gap
//!
//! `gdserver3d/src/compositor.rs` defined `Compositor`, `CompositorEffect` and
//! `EffectCallbackType` — and **nothing called any of it**. Outside that file
//! the only references were the `lib.rs` re-export, the ClassDB registration
//! and tests. Neither renderer mentioned a compositor, so authoring one changed
//! not a single pixel.
//!
//! The data model was also inert by construction: `CompositorEffect` had flags
//! describing which buffers a pass wanted and **no way to invoke anything**.
//! Adding `callback` is what turns the description into a pass.
//!
//! ⚠️ **Effects hold Rust callbacks, which a `.tscn` cannot express.** The scene
//! supplies the geometry and the `WorldEnvironment`; the compositor is installed
//! by the caller through `render_editor_3d_frame_with_compositor`. That is why
//! the acceptance says "registers a Rust test effect".

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use gdcore::math::Color;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{
    render_editor_3d_frame, render_editor_3d_frame_with_compositor,
};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdserver3d::compositor::{
    Compositor, CompositorEffect, CompositorEffectCallback, CompositorRenderData,
    EffectCallbackType,
};

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn viewport() -> Viewport3D {
    let mut vp = Viewport3D::default();
    vp.width = 64;
    vp.height = 64;
    vp
}

/// A `WorldEnvironment` plus a floor, so the frame has real content to tint
/// rather than a flat backdrop.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="World" type="WorldEnvironment" parent="."]

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"
"#;

/// Counts its invocations and forces the whole colour buffer to one value.
///
/// Both halves matter: the count proves the callback ran **exactly once** per
/// frame rather than per effect-stage or per pixel-pass, and the write proves it
/// received a buffer whose mutation survives into the output.
struct TintEffect {
    calls: AtomicUsize,
    stages: std::sync::Mutex<Vec<EffectCallbackType>>,
    tint: Color,
}

impl TintEffect {
    fn new(tint: Color) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            stages: std::sync::Mutex::new(Vec::new()),
            tint,
        })
    }
}

impl CompositorEffectCallback for TintEffect {
    fn render_callback(&self, stage: EffectCallbackType, data: &mut CompositorRenderData<'_>) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.stages.lock().expect("stages").push(stage);
        for px in data.color.iter_mut() {
            *px = self.tint;
        }
    }
}

// ---------------------------------------------------------------------------
// The bead's acceptance
// ---------------------------------------------------------------------------

/// **The acceptance test.** One effect at POST_TRANSPARENT runs once per frame
/// and its colour-buffer mutation appears in the output; without it the frame is
/// untinted.
#[test]
fn compositor_effect_runs_in_frame() {
    let tree = load(SCENE);
    let vp = viewport();

    // The control, rendered FIRST: the same scene with no compositor. Captured
    // rather than assumed, because "the tint appeared" means nothing unless the
    // frame did not already look like that.
    let plain = render_editor_3d_frame(&vp, &tree);

    let tint = Color::new(1.0, 0.0, 1.0, 1.0);
    let effect = TintEffect::new(tint);
    let mut compositor = Compositor::new();
    compositor.add_effect(
        CompositorEffect::new(EffectCallbackType::PostTransparent).with_callback(effect.clone()),
    );

    let composited = render_editor_3d_frame_with_compositor(&vp, &tree, Some(compositor));

    // -- ran exactly once ----------------------------------------------------
    // An exact count, not `>= 1`: the invocation walks five stages, so a
    // dispatch that ignored `effect_callback_type` and ran every effect at
    // every boundary would fire five times and still "work" visually.
    assert_eq!(
        effect.calls.load(Ordering::SeqCst),
        1,
        "the effect must run exactly once per rendered frame"
    );
    assert_eq!(
        effect.stages.lock().expect("stages").as_slice(),
        &[EffectCallbackType::PostTransparent],
        "and at the stage it was registered for, not another"
    );

    // -- the mutation reached the output -------------------------------------
    assert!(
        !composited.pixels.is_empty(),
        "the frame must have pixels to inspect"
    );
    assert_eq!(
        plain.pixels.len(),
        composited.pixels.len(),
        "both frames are the same size, so they compare pixel for pixel"
    );

    // ⚠️ NOT "every pixel is the tint", and NOT "every changed pixel is the
    // tint" either. The effect writes the whole colour buffer, but the editor
    // draws its overlays AFTER the 3D pass and blends them: the ground grid is
    // anti-aliased and fades with distance (`viewport_3d_render.rs` —
    // "Drawn faint; far lines fade", compositing through `Raster::blend`).
    //
    // A partially-covered overlay pixel is therefore a blend of the grid colour
    // with whatever lay beneath it, so it lands on a DIFFERENT value in the two
    // renders and is neither frame's tint. This assertion used to require every
    // changed pixel to equal the tint exactly and failed on pixel 640 — row 10,
    // column 0 of the 64x64 frame — which came out
    // `(0.977, 0.035, 0.983)`: magenta pulled slightly toward the grid line.
    // The giveaway is that the two channels that were both 1.0 in the tint came
    // back DIFFERENT (0.977 vs 0.983), which no per-channel colour transform can
    // do — only a blend with something else can.
    //
    // 🔑 The invariant that survives all of that, without needing to know where
    // the grid drew: render the same frame with TWO different tints. A pixel the
    // overlay never touched is written by the effect and nothing else, so it
    // equals its own tint in each render. A blended pixel equals neither. So the
    // set of pixels matching tint A must be EXACTLY the set matching tint B —
    // and it must not be empty.
    // Any pixel at all must have moved between the two renders. This is the
    // coarsest possible statement of "the callback's buffer was not a copy",
    // and it is kept because it fails loudly and early if the effect never ran.
    let changed: Vec<usize> = (0..plain.pixels.len())
        .filter(|&i| plain.pixels[i] != composited.pixels[i])
        .collect();
    assert!(
        !changed.is_empty(),
        "the effect's write must reach the output — no pixel changed at all, \
         which is what a callback handed a copy of the buffer would produce"
    );

    let other = Color::new(0.0, 1.0, 0.0, 1.0);
    let second = TintEffect::new(other);
    let mut compositor2 = Compositor::new();
    compositor2.add_effect(
        CompositorEffect::new(EffectCallbackType::PostTransparent).with_callback(second.clone()),
    );
    let composited2 = render_editor_3d_frame_with_compositor(&vp, &tree, Some(compositor2));

    let matches_tint = |frame: &[Color], c: Color| -> Vec<usize> {
        (0..frame.len())
            .filter(|&i| frame[i].r == c.r && frame[i].g == c.g && frame[i].b == c.b)
            .collect()
    };
    let a = matches_tint(&composited.pixels, tint);
    let b = matches_tint(&composited2.pixels, other);

    assert!(
        !a.is_empty(),
        "no pixel came out exactly the tint, so the effect's write never \
         reached the output unblended anywhere"
    );
    assert_eq!(
        a, b,
        "the pixels an effect owns must not depend on WHICH colour it wrote: \
         every pixel the editor overlay did not touch is written by the effect \
         and nothing else, so both renders must own exactly the same set"
    );
    // ...and that set must be most of the frame, or "the effect ran" would be
    // satisfied by a write that reached a single stray pixel.
    assert!(
        a.len() * 2 > composited.pixels.len(),
        "the effect writes the whole colour buffer, so most of the frame must \
         come out exactly the tint; only {} of {} pixels did",
        a.len(),
        composited.pixels.len()
    );
}

/// Removing the effect restores the untinted frame.
///
/// The other direction of the acceptance, and the one that catches a tint
/// leaking into state that outlives the frame.
#[test]
fn compositor_effect_runs_in_frame_removed_effect_restores_frame() {
    let tree = load(SCENE);
    let vp = viewport();

    let plain = render_editor_3d_frame(&vp, &tree);

    let effect = TintEffect::new(Color::new(0.0, 1.0, 0.0, 1.0));
    let mut compositor = Compositor::new();
    compositor.add_effect(
        CompositorEffect::new(EffectCallbackType::PostTransparent).with_callback(effect.clone()),
    );
    let _ = render_editor_3d_frame_with_compositor(&vp, &tree, Some(compositor));

    let after = render_editor_3d_frame_with_compositor(&vp, &tree, None);
    assert_eq!(
        plain.pixels, after.pixels,
        "with the compositor removed the frame must match the original exactly"
    );
    assert_eq!(
        effect.calls.load(Ordering::SeqCst),
        1,
        "and the removed effect must not run again"
    );
}

/// A disabled effect is skipped, and one registered at another stage does not
/// fire at POST_TRANSPARENT.
///
/// These are the two filters `effects_for_stage` applies; without a test they
/// are assertions about code nobody exercises.
#[test]
fn compositor_effect_runs_in_frame_skips_disabled_and_other_stages() {
    let tree = load(SCENE);
    let vp = viewport();

    let disabled = TintEffect::new(Color::new(1.0, 0.0, 0.0, 1.0));
    let mut effect =
        CompositorEffect::new(EffectCallbackType::PostTransparent).with_callback(disabled.clone());
    effect.enabled = false;

    let other_stage = TintEffect::new(Color::new(0.0, 0.0, 1.0, 1.0));

    let mut compositor = Compositor::new();
    compositor.add_effect(effect);
    compositor.add_effect(
        CompositorEffect::new(EffectCallbackType::PreOpaque).with_callback(other_stage.clone()),
    );

    let _ = render_editor_3d_frame_with_compositor(&vp, &tree, Some(compositor));

    assert_eq!(
        disabled.calls.load(Ordering::SeqCst),
        0,
        "a disabled effect must never run"
    );
    // The PreOpaque one DOES run — it is registered at a real stage — which is
    // what makes the disabled assertion above meaningful rather than a test in
    // which nothing was ever going to fire.
    assert_eq!(
        other_stage.calls.load(Ordering::SeqCst),
        1,
        "an enabled effect at another stage still runs, once, at its own stage"
    );
    assert_eq!(
        other_stage.stages.lock().expect("stages").as_slice(),
        &[EffectCallbackType::PreOpaque],
        "at PreOpaque specifically"
    );
}
