//! pat-s3rrv.55 — Audio3D attenuation, emission cone and Doppler, from a scene.
//!
//! The maths for all three lived in `gdaudio::spatial` and was unit-tested
//! there. What this pins is the part that was missing: that a scene-tree source
//! actually *reaches* it every frame. Doppler in particular was a complete
//! no-op — `doppler_pitch_scale` had zero callers, so `doppler_tracking` was an
//! authored property that changed nothing.
//!
//! Attenuation is asserted against Godot's own formula rather than a recorded
//! number, so the test states the contract instead of the current behaviour.

use gdcore::math::Vector3;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

const SR: u32 = 48_000;

/// Godot's `AudioStreamPlayer3D` attenuation models, by their enum ordinal.
const INVERSE_DISTANCE: i64 = 0;
const INVERSE_SQUARE_DISTANCE: i64 = 1;
const LOGARITHMIC: i64 = 2;

fn linear_to_db(v: f32) -> f32 {
    20.0 * v.max(1e-9).log10()
}

fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// Godot's attenuation, transcribed from `audio_stream_player_3d.cpp:235-245`,
/// so the assertions compare against the SPEC rather than against whatever
/// Patina currently computes.
///
/// ⚠️ The logarithmic model uses the NATURAL log. Godot's `Math::log` is `ln`,
/// not `log10`, so the curve is `-20 * ln(d/u)` dB — far steeper than the
/// `-20 * log10(d/u)` one might assume from the name (at d/u = 4 that is
/// -27.7 dB versus -12.0 dB). Writing this from memory of the word
/// "logarithmic" instead of from the source gets it wrong by 2.3x in dB.
fn godot_attenuation_linear(model: i64, distance: f32, unit_size: f32) -> f32 {
    let d = (distance / unit_size).max(1e-6);
    match model {
        INVERSE_DISTANCE => db_to_linear(linear_to_db(1.0 / d)),
        INVERSE_SQUARE_DISTANCE => db_to_linear(linear_to_db(1.0 / (d * d))),
        LOGARITHMIC => db_to_linear(-20.0 * d.ln()),
        _ => 1.0,
    }
}

/// A constant full-scale mono buffer: any gain change shows up directly in the
/// mixed peak, with no waveform shape to reason about.
fn dc_buffer() -> gdaudio::AudioBuffer {
    gdaudio::AudioBuffer {
        sample_rate: SR,
        channels: 1,
        samples: vec![1.0; SR as usize],
    }
}

/// A 12 kHz tone: the period-4 pattern `+1 +1 -1 -1` at 48 kHz.
///
/// 🔑 The emission cone is a HIGH-SHELF cut, not a volume cut, so it is
/// invisible on the DC buffer above — a shelf leaves 0 Hz at unity gain by
/// definition. That is why the cone assertion could never move while it used
/// `dc_buffer`, independently of whether the filter was applied at all.
///
/// ⚠️ Not a Nyquist (`+1 -1`) pattern, which would be the strongest signal for
/// a shelf: the mixer runs at 44.1 kHz and this buffer is 48 kHz, so everything
/// is resampled at a ratio of 1.088 and a Nyquist tone would alias into
/// garbage. 12 kHz survives resampling and still sits deep in the shelf's
/// stopband.
fn hf_buffer() -> gdaudio::AudioBuffer {
    gdaudio::AudioBuffer {
        sample_rate: SR,
        channels: 1,
        samples: (0..SR as usize)
            .map(|i| if (i / 2) % 2 == 0 { 1.0 } else { -1.0 })
            .collect(),
    }
}

/// RMS of the combined stereo magnitude.
///
/// Used instead of the peak wherever the signal is a TONE rather than DC: the
/// resampler ripples the peak of a 12 kHz tone frame to frame, while its energy
/// is steady.
fn combined_rms(mix: &[f32]) -> f32 {
    let frames: Vec<f32> = mix
        .chunks_exact(2)
        .map(|f| f[0] * f[0] + f[1] * f[1])
        .collect();
    (frames.iter().sum::<f32>() / frames.len().max(1) as f32).sqrt()
}

/// Recovers the source's GAIN from the interleaved stereo mix.
///
/// ⚠️ A per-channel peak is not the gain. Panning is equal-power
/// (`gdaudio::spatial::compute_spatial` uses `angle.cos()` / `angle.sin()`), so
/// a centred source puts `gain / sqrt(2)` in each channel: reading one channel
/// under-reports by 0.7071 and makes a perfectly correct attenuation curve look
/// wrong by exactly that factor. Worse for the cone case, where the two runs sit
/// at DIFFERENT angles — a hard-panned source has ~gain in one channel while a
/// centred one has gain/sqrt(2), so a per-channel comparison can rank an
/// attenuated source as louder.
///
/// `sqrt(L^2 + R^2)` recovers the gain at any pan position, because
/// `cos^2 + sin^2 == 1`.
fn combined_peak(mix: &[f32]) -> f32 {
    mix.chunks_exact(2)
        .map(|f| (f[0] * f[0] + f[1] * f[1]).sqrt())
        .fold(0.0f32, f32::max)
}

/// Builds a tree with a current `Camera3D` at the origin (the listener) and an
/// autoplaying `AudioStreamPlayer3D` at `pos`.
fn scene_3d_with(
    pos: Vector3,
    extra: &[(&str, Variant)],
    buffer: gdaudio::AudioBuffer,
) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut cam = Node::new("Camera", "Camera3D");
    cam.set_property("current", Variant::Bool(true));
    tree.add_child(root, cam).unwrap();

    let mut sfx = Node::new("Sfx", "AudioStreamPlayer3D");
    sfx.set_property("autoplay", Variant::Bool(true));
    sfx.set_property("position", Variant::Vector3(pos));
    for (key, value) in extra {
        sfx.set_property(key, value.clone());
    }
    let sfx_id = tree.add_child(root, sfx).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_stream_buffer(sfx_id, buffer);
    (ml, sfx_id)
}

fn scene_3d(pos: Vector3, extra: &[(&str, Variant)]) -> (MainLoop, NodeId) {
    scene_3d_with(pos, extra, dc_buffer())
}

fn run(ml: &mut MainLoop, frames: usize) -> Vec<f32> {
    for _ in 0..frames {
        ml.step(1.0 / 60.0);
    }
    ml.audio_bridge().last_mix().to_vec()
}

/// **The acceptance test.**
#[test]
fn audio3d_attenuation_cone_doppler() {
    // -- 1. All three attenuation models match Godot's formula ---------------
    //
    // unit_size 4 with the source 16 units out puts d/u at 4, which is far
    // enough into each curve that the three models give clearly different
    // gains — a source at d == u would leave all three at unity and the
    // assertion could not tell them apart.
    let unit_size = 4.0f32;
    let distance = 16.0f32;
    for model in [INVERSE_DISTANCE, INVERSE_SQUARE_DISTANCE, LOGARITHMIC] {
        let (mut ml, _) = scene_3d(
            Vector3::new(0.0, 0.0, -distance),
            &[
                ("unit_size", Variant::Float(unit_size as f64)),
                ("attenuation_model", Variant::Int(model)),
                // Off by default in Godot; an unbounded max_db would let the
                // clamp mask a wrong curve.
                ("max_db", Variant::Float(3.0)),
            ],
        );
        let mix = run(&mut ml, 3);
        assert!(
            !mix.is_empty(),
            "model {model}: the source must reach the mixer"
        );

        let expected = godot_attenuation_linear(model, distance, unit_size);
        let got = combined_peak(&mix);
        assert!(
            (got - expected).abs() < 1e-3,
            "attenuation model {model} at d={distance} u={unit_size} must match \
             Godot's curve: expected {expected:.6}, got {got:.6}",
        );
    }

    // -- 2. Outside the emission cone the source is attenuated ---------------
    //
    // The cone points along the source's forward axis. Both runs put the
    // listener at the SAME distance, so the only difference is angle — a test
    // that moved the source would confound the cone with attenuation.
    let cone = [
        ("emission_angle_enabled", Variant::Bool(true)),
        ("emission_angle_degrees", Variant::Float(30.0)),
        (
            "emission_angle_filter_attenuation_db",
            Variant::Float(-24.0),
        ),
    ];
    // ⚠️ The source at +Z, NOT -Z. A Node3D's forward is its -Z basis column,
    // so a source parked at (0, 0, -10) points AWAY from a listener at the
    // origin — 180° off-axis. The original fixture put both runs outside the
    // cone, which is one of the two reasons they returned identical values.
    let mut on_axis_props = cone.to_vec();
    on_axis_props.push(("unit_size", Variant::Float(4.0)));
    let (mut on_axis_ml, _) =
        scene_3d_with(Vector3::new(0.0, 0.0, 10.0), &on_axis_props, hf_buffer());
    let on_axis = combined_rms(&run(&mut on_axis_ml, 3));

    // Same radius, rotated 90° — well outside a 30-degree cone.
    let mut off_axis_props = cone.to_vec();
    off_axis_props.push(("unit_size", Variant::Float(4.0)));
    let (mut off_axis_ml, _) =
        scene_3d_with(Vector3::new(10.0, 0.0, 0.0), &off_axis_props, hf_buffer());
    let off_axis = combined_rms(&run(&mut off_axis_ml, 3));

    // `unit_size` is 4, not 100: at d = 10 that is a gain of 0.4, comfortably
    // under `max_db`. The original 100 made the raw gain +20 dB, so BOTH runs
    // were pinned at the 3 dB ceiling (1.412538) — the other reason they
    // matched, and it would have masked any real difference.
    //
    // `max_distance` is off by default, so the distance filter contributes 0 dB
    // and the cone is the ONLY shelf term: on-axis is an exact bypass, off-axis
    // is the full -24 dB. At 12 kHz that is |H| = 0.068, so the threshold below
    // has 7x of margin and still cannot be satisfied by "no filtering".
    assert!(
        off_axis < on_axis * 0.5,
        "a listener outside the 30-degree emission cone must be attenuated \
         relative to on-axis: on {on_axis:.6} vs off {off_axis:.6} (expected \
         about {:.6}). Equal values mean the cone is not consulted at all.",
        on_axis * 0.068,
    );
    assert!(
        on_axis > 1e-4,
        "the on-axis run must actually be audible, else the comparison above is \
         vacuous; got {on_axis:.6}",
    );

    // -- 3. Doppler raises the pitch of an approaching source ----------------
    //
    // This is the clause that was pure no-op: `doppler_pitch_scale` existed with
    // zero callers. Velocity comes from the CHANGE in position between frames,
    // so the source has to actually move across steps — a static scene can
    // never exercise it.
    let (mut ml, sfx) = scene_3d(
        Vector3::new(0.0, 0.0, -100.0),
        &[
            // 1 = IdleStep.
            ("doppler_tracking", Variant::Int(1)),
            ("unit_size", Variant::Float(1000.0)),
        ],
    );
    // First step establishes the previous position; no velocity exists yet.
    ml.step(1.0 / 60.0);
    let baseline = ml.audio_bridge_mut().playback_pitch_scale(sfx);

    // ⚠️ 3 units per 1/60 s, NOT 60. At 60 the source travels -100 -> -40 -> 20
    // -> 80: it passes THROUGH the listener, so the final step is a RECESSION
    // and the pitch read afterwards is below baseline — the assertion below
    // would fail on a perfectly correct Doppler implementation. 3 units keeps
    // it approaching the whole way (-97, -94, -91).
    //
    // 3 units per frame is 180 units/s, about half the 343 m/s speed of sound,
    // giving 343/(343-180) = 2.10x. That exercises the real formula rather than
    // its supersonic clamp, which would return a flat 4.0 and pass even if the
    // ratio were wrong.
    for frame in 1..4 {
        let z = -100.0 + 3.0 * frame as f32;
        if let Some(node) = ml.tree_mut().get_node_mut(sfx) {
            node.set_property("position", Variant::Vector3(Vector3::new(0.0, 0.0, z)));
        }
        ml.step(1.0 / 60.0);
    }
    let approaching = ml.audio_bridge_mut().playback_pitch_scale(sfx);

    assert!(
        approaching > baseline,
        "a source moving toward the listener with doppler_tracking enabled must \
         raise the playback rate: baseline {baseline:?} vs approaching \
         {approaching:?}. Equal values mean `doppler_pitch_scale` is still \
         being computed by nobody.",
    );
}

/// With `doppler_tracking` disabled the same motion must change nothing — the
/// control that stops the assertion above from passing on any incidental
/// per-frame pitch drift.
#[test]
fn doppler_disabled_leaves_pitch_alone() {
    let (mut ml, sfx) = scene_3d(
        Vector3::new(0.0, 0.0, -100.0),
        &[
            ("doppler_tracking", Variant::Int(0)), // Disabled
            ("unit_size", Variant::Float(1000.0)),
        ],
    );
    ml.step(1.0 / 60.0);
    let before = ml.audio_bridge_mut().playback_pitch_scale(sfx);

    for frame in 1..4 {
        // The same approach as the test above, so this really is its control.
        let z = -100.0 + 3.0 * frame as f32;
        if let Some(node) = ml.tree_mut().get_node_mut(sfx) {
            node.set_property("position", Variant::Vector3(Vector3::new(0.0, 0.0, z)));
        }
        ml.step(1.0 / 60.0);
    }
    let after = ml.audio_bridge_mut().playback_pitch_scale(sfx);

    assert_eq!(
        before, after,
        "doppler_tracking = Disabled must leave the playback rate untouched",
    );
}
