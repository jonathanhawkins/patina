//! pat-mgixk.4 acceptance: a scene's `AudioStreamPlayer` reaches the ring the
//! browser's `AudioWorkletProcessor` drains.
//!
//! Godot's web export runs exactly ONE `AudioServer` and lets an
//! `AudioWorkletProcessor` pull mixed frames from it
//! (`platform/web/audio_driver_web.cpp`), so every player in a running scene is
//! audible once a gesture has unlocked the context.
//!
//! Patina's web runtime had *two* mixers and the page drained the wrong one.
//! `patina-web`'s `WebAudioEngine` owned a private `AudioServer` reachable only
//! from `playTone`/`playEncoded`, while the scene's `MainLoop` owned an
//! `AudioServerBridge` with no output attached at all — `grep -i audio` over
//! `browser_runtime.rs` and `engine.rs` matched nothing. `process()` therefore
//! mixed the whole scene into `last_mix` and dropped it, and every
//! `AudioStreamPlayer` in a web game was silent however correct its scene was.
//!
//! This drives the real chain, with no `playTone` and no `set_stream_buffer`
//! injection anywhere:
//!
//! ```text
//! .wav on disk + .tscn text -> PackedScene -> SceneTree
//!   -> WebEngine::frame (what requestAnimationFrame hands its callback)
//!     -> MainLoop::step -> AudioServerBridge::process
//!       -> SharedWebAudioOutput ring -> SceneAudioRing::pull_block
//! ```
//!
//! `pull_block` is the function the worklet's pull resolves to: `WebAudio`'s
//! `pullBlock` binding sums exactly this into what it hands the processor.
//!
//! ⚠️ The two halves of the claim are not interchangeable. "Audio comes out"
//! alone would also pass if `pull_block` quietly mixed a server of its own,
//! which is the bug being fixed; so this also asserts that **stopping the
//! player silences the ring**, and that a pull taken before any frame has run
//! is silent. Both are only true if the frames really come from the MainLoop.

use gdaudio::wav::build_wav_bytes;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;
use patina_web::engine::WebEngine;

/// A rate a browser might pick for its `AudioContext`, deliberately different
/// from the mix rate so a missing resample step cannot pass unnoticed.
const CONTEXT_RATE: u32 = 48_000;
/// Godot's `audio/driver/mix_rate` default, and what the demo shell passes.
const MIX_RATE: u32 = 44_100;
/// Frames an `AudioWorkletProcessor` renders per `process()` call.
const QUANTUM: usize = 128;
/// A browser's clock does not start at zero.
const ORIGIN: f64 = 1234.5;
/// One 60 Hz animation-frame interval, in milliseconds.
const FRAME_MS: f64 = 1000.0 / 60.0;
/// Quanta to drain per simulated frame.
///
/// One 60 Hz frame mixes `1/60 * 44100` = 735 frames, which the output
/// resamples to 800 frames at 48 kHz — 6.25 quanta. Six keeps the ring from
/// running dry without letting it grow toward its quarter-second cap.
const QUANTA_PER_FRAME: usize = 6;

/// A scene exactly as Godot writes one: an `[ext_resource]` header plus a
/// player whose `stream` references it. No inline buffer, no script.
const SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="AudioStreamWAV" path="res://beep.wav" id="1_beep"]

[node name="World" type="Node2D"]

[node name="Beeper" type="AudioStreamPlayer" parent="."]
stream = ExtResource("1_beep")
autoplay = true
"#;

/// `seconds` of loud 440 Hz mono at the mix rate, as a real RIFF/WAV file.
fn tone_wav(seconds: f32) -> Vec<u8> {
    let frames = (seconds * MIX_RATE as f32) as u32;
    let mut pcm = Vec::with_capacity(frames as usize * 2);
    for n in 0..frames {
        let t = n as f32 / MIX_RATE as f32;
        let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.8;
        pcm.extend_from_slice(&((s * i16::MAX as f32) as i16).to_le_bytes());
    }
    // audio_format 1 = PCM.
    build_wav_bytes(MIX_RATE, 1, 16, 1, &pcm)
}

/// A scratch directory, removed on drop.
///
/// Hand-rolled rather than `tempfile`: this crate has no dev-dependencies, and
/// adding one would rewrite `Cargo.lock`, which the worker lane cannot
/// regenerate. The pid makes it unique per test binary.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("patina-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the scratch directory");
        Self(dir)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Root-mean-square of a block — "how loud was this", rather than a single
/// peak that a zero crossing could hide.
fn rms(block: &[f32]) -> f32 {
    if block.is_empty() {
        return 0.0;
    }
    (block.iter().map(|s| s * s).sum::<f32>() / block.len() as f32).sqrt()
}

#[test]
fn web_runtime_scene_audio_reaches_worklet_ring() {
    let scratch = Scratch::new("web-scene-audio");
    std::fs::write(scratch.path().join("beep.wav"), tone_wav(4.0)).expect("write the wav");

    let scene = PackedScene::from_tscn(SCENE).expect("parse the .tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance the scene");

    let mut engine = WebEngine::new(tree);
    engine
        .main_loop_mut()
        .audio_bridge_mut()
        .set_base_dir(scratch.path().to_path_buf());

    // The call this whole bead is about. Before it, the bridge has no output.
    let mut ring = engine.attach_audio_output(CONTEXT_RATE, MIX_RATE);

    assert_eq!(
        ring.context_rate(),
        CONTEXT_RATE,
        "the ring must hold samples at the rate the browser chose, not the mix rate"
    );
    assert_eq!(
        engine
            .main_loop()
            .audio_bridge()
            .server()
            .output_sample_rate(),
        MIX_RATE,
        "audio/driver/mix_rate must reach the bridge's server"
    );
    let channels = ring.channels() as usize;
    assert!(channels >= 1, "the ring must report a channel count");

    // ⚠️ A pull before any frame has run must be silent. If this is loud, the
    // pull is mixing a server of its own and the rest of this test proves
    // nothing about the MainLoop.
    let cold = ring.pull_block(QUANTUM);
    assert_eq!(
        cold.len(),
        QUANTUM * channels,
        "a pull must always return a full block — `process()` has to fill its output buffer"
    );
    assert_eq!(
        rms(&cold),
        0.0,
        "audio appeared without a frame ever stepping, so `pull_block` is mixing \
         something the scene does not drive"
    );

    // --- the scene's autoplaying player reaches the ring -------------------
    //
    // The first callback only establishes the time origin; it is the boundary,
    // not a step.
    assert!(
        engine.frame(ORIGIN).is_none(),
        "the first animation-frame callback establishes the origin"
    );
    let mut stamp = ORIGIN;
    let mut loudest = 0.0f32;
    let mut stepped = 0usize;
    for _ in 0..12 {
        stamp += FRAME_MS;
        if engine.frame(stamp).is_some() {
            stepped += 1;
        }
        for _ in 0..QUANTA_PER_FRAME {
            let block = ring.pull_block(QUANTUM);
            assert_eq!(block.len(), QUANTUM * channels);
            loudest = loudest.max(rms(&block));
        }
    }
    assert!(stepped > 0, "the callbacks must actually step the MainLoop");
    assert!(
        loudest > 0.01,
        "the scene's autoplaying AudioStreamPlayer never reached the worklet ring \
         (loudest block rms {loudest}) — AudioServerBridge::process is still \
         discarding its mix"
    );
    assert_eq!(
        engine.main_loop().audio_bridge().active_stream_count(),
        1,
        "the scene's autoplay voice should be live"
    );
    assert!(
        engine.main_loop().audio_bridge().last_mix().is_empty(),
        "with an output attached the bridge must write THROUGH it rather than \
         retaining the mix — a non-empty last_mix means the output branch was \
         never taken"
    );
    assert_eq!(
        ring.frames_pulled(),
        ((12 * QUANTA_PER_FRAME + 1) * QUANTUM) as u64,
        "every pull, including the cold one, counts"
    );
    assert!(
        engine.main_loop().audio_bridge().stream_errors().is_empty(),
        "the stream must have loaded from the file the scene names: {:?}",
        engine.main_loop().audio_bridge().stream_errors()
    );

    // --- stopping the player silences the ring -----------------------------
    //
    // This is the half that proves the audio came from the MainLoop. A pull
    // that mixed its own source would keep producing sound here.
    let player = engine
        .tree()
        .get_node_by_path("/root/World/Beeper")
        .expect("the scene's AudioStreamPlayer");
    engine
        .tree_mut()
        .get_node_mut(player)
        .expect("the player node")
        .set_property("playing", Variant::Bool(false));

    stamp += FRAME_MS;
    engine.frame(stamp);
    assert_eq!(
        engine.main_loop().audio_bridge().active_stream_count(),
        0,
        "`playing = false` must stop the voice"
    );

    // Everything mixed BEFORE the stop is still queued. Drain the ring dry
    // first: asserting silence here without this would read the tail of the
    // tone and fail for the right reason at the wrong time.
    let mut drains = 0;
    while ring.queued_frames() > 0 && drains < 10_000 {
        ring.pull_block(QUANTUM);
        drains += 1;
    }
    assert!(drains < 10_000, "the ring never drained");

    for frame in 0..10 {
        stamp += FRAME_MS;
        engine.frame(stamp);
        while ring.queued_frames() >= QUANTUM {
            let block = ring.pull_block(QUANTUM);
            let loud = rms(&block);
            assert!(
                loud < 1e-5,
                "frame {frame}: the stopped player is still audible (rms {loud}), so the \
                 frames the worklet gets are not the MainLoop's"
            );
        }
    }
}
