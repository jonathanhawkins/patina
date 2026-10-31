//! pat-mgixk.4 (item 4): an `AudioStreamMP3` in a web scene becomes audible
//! through the browser's own MP3 decoder.
//!
//! # The gap this closes
//!
//! Godot's web export has no MP3 codec of its own; it leans on the browser's.
//! Patina decodes ISO MP3 with `minimp3`, which builds C sources through `cc`
//! and is therefore gated to non-wasm targets in `gdaudio/Cargo.toml`. So
//! `decode_mp3_frames` on wasm32 is a stub that returns `UnsupportedFormat`,
//! and **every `AudioStreamMP3` in a web game played silence** — the three
//! other halves of pat-mgixk.4 (the shared ring, the `attach_output` call, the
//! mix rate) made `AudioStreamPlayer` audible on the web but could not make an
//! MP3 decode.
//!
//! # What is driven
//!
//! ```text
//! browser decodeAudioData -> registerDecodedMp3 (BrowserMp3Decoder)
//!   -> installed as gdaudio's host MP3 decoder
//!     .mp3 on disk + .tscn text -> PackedScene -> SceneTree
//!       -> WebEngine::frame (what requestAnimationFrame hands its callback)
//!         -> MainLoop::step -> AudioServerBridge::process -> AudioStreamLoader
//!           -> decode_mp3 -> the hook -> ring -> SceneAudioRing::pull_block
//! ```
//!
//! `pull_block` is what the worklet's pull resolves to.
//!
//! # Why the fixture is deliberately an *undecodable* MP3
//!
//! The file on disk passes `gdaudio::is_mp3` (it carries an `ID3` tag) but
//! holds no decodable frame, so **no linked codec on any target can turn it
//! into audio** — which is exactly the browser's position with respect to every
//! real ISO MP3.
//!
//! That is the point. A real `.mp3` fixture would decode natively through
//! `minimp3` and this test would pass with the hook ripped out, proving nothing
//! about the web build. With these bytes the only possible source of sound is
//! the registered PCM, so "audio came out" *is* "the browser-decode path
//! worked". The companion test below removes the registration and asserts
//! silence, which pins that claim from the other side.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use patina_web::engine::WebEngine;
use patina_web::mp3_bridge::BrowserMp3Decoder;

/// A rate a browser might pick for its `AudioContext`, deliberately different
/// from the mix rate so a missing resample step cannot pass unnoticed.
const CONTEXT_RATE: u32 = 48_000;
/// Godot's `audio/driver/mix_rate` default, and what the demo shell passes.
const MIX_RATE: u32 = 44_100;
/// The rate `decodeAudioData` hands PCM back at — the context's, not the
/// engine's. Different from `MIX_RATE` on purpose: the stream has to be
/// resampled on its way through the mixer, and a hook that lied about its rate
/// would be heard as the wrong pitch rather than as silence.
const DECODED_RATE: u32 = 48_000;
/// Frames an `AudioWorkletProcessor` renders per `process()` call.
const QUANTUM: usize = 128;
/// A browser's clock does not start at zero.
const ORIGIN: f64 = 9876.5;
/// One 60 Hz animation-frame interval, in milliseconds.
const FRAME_MS: f64 = 1000.0 / 60.0;
/// Quanta to drain per simulated frame — see the sibling WAV acceptance for the
/// arithmetic; six keeps the ring from running dry without letting it grow.
const QUANTA_PER_FRAME: usize = 6;

/// A one-player scene naming `res://music.mp3` with autoplay on.
///
/// The bare-string `stream = "res://..."` form rather than an `[ext_resource]`
/// block: that is the shape pat-s3rrv.47's green
/// `audio_import_remap_and_mp3_stream_plays` pins for MP3 specifically, so a
/// failure here cannot be the scene syntax.
const SCENE: &str = r#"[gd_scene format=3]

[node name="World" type="Node"]

[node name="Music" type="AudioStreamPlayer" parent="."]
stream = "res://music.mp3"
autoplay = true
"#;

/// Serializes the tests that install into `gdaudio`'s process-global decoder
/// slot, and empties it again on the way out.
///
/// Bound ONCE per test rather than cleared at the top of each: a per-test reset
/// is a re-arm, and under a thread-per-test runner two tests would still
/// interleave between the clear and the install. Poison-tolerant so one failure
/// does not cascade.
static DECODER_SLOT: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[must_use]
struct DecoderSlot(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

impl DecoderSlot {
    fn acquire() -> Self {
        Self(DECODER_SLOT.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl Drop for DecoderSlot {
    fn drop(&mut self) {
        gdaudio::decode::clear_mp3_decoder();
    }
}

/// A scratch directory, removed on drop.
///
/// Hand-rolled rather than `tempfile`: this crate has no dev-dependencies, and
/// adding one would rewrite `Cargo.lock`, which the worker lane cannot
/// regenerate. The pid plus a tag makes it unique per test binary and per test.
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

/// Bytes that pass `gdaudio::is_mp3` — an `ID3v2` header — but hold no
/// decodable frame.
///
/// `minimp3` reports the tag as skipped data and then runs out, so
/// `decode_mp3_frames` errors on native; on wasm32 it is a stub that errors
/// regardless. Either way nothing but the hook can produce audio from this.
fn undecodable_mp3() -> Vec<u8> {
    // "ID3", version 4.0, no flags, then a 10-byte size-syncsafe body.
    let mut data = b"ID3\x04\x00\x00\x00\x00\x00\x0a".to_vec();
    data.extend_from_slice(&[0u8; 10]);
    data
}

/// `seconds` of loud 440 Hz as channel-major stereo PCM, the shape
/// `decodeAudioData` hands JS back (one array per channel, concatenated).
fn tone_planar(seconds: f32) -> Vec<f32> {
    let frames = (seconds * DECODED_RATE as f32) as usize;
    let plane: Vec<f32> = (0..frames)
        .map(|n| {
            let t = n as f32 / DECODED_RATE as f32;
            (t * 440.0 * std::f32::consts::TAU).sin() * 0.8
        })
        .collect();
    // Left then right: `register_planar` is what has to interleave them.
    let mut planar = Vec::with_capacity(frames * 2);
    planar.extend_from_slice(&plane);
    planar.extend_from_slice(&plane);
    planar
}

/// Root-mean-square of a block — "how loud was this", rather than a single peak
/// a zero crossing could hide.
fn rms(block: &[f32]) -> f32 {
    if block.is_empty() {
        return 0.0;
    }
    (block.iter().map(|s| s * s).sum::<f32>() / block.len() as f32).sqrt()
}

/// Instances `SCENE` against `dir` and attaches the worklet ring.
fn booted(dir: &std::path::Path) -> (WebEngine, patina_web::audio::SceneAudioRing) {
    let scene = PackedScene::from_tscn(SCENE).expect("parse the .tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance the scene");

    let mut engine = WebEngine::new(tree);
    engine
        .main_loop_mut()
        .audio_bridge_mut()
        .set_base_dir(dir.to_path_buf());
    let ring = engine.attach_audio_output(CONTEXT_RATE, MIX_RATE);
    (engine, ring)
}

/// **The acceptance test.** A scene's MP3 reaches the worklet ring, decoded by
/// the browser.
#[test]
fn web_mp3_browser_decode_reaches_worklet_ring() {
    let _slot = DecoderSlot::acquire();
    let scratch = Scratch::new("web-mp3-decoded");
    let mp3 = undecodable_mp3();
    std::fs::write(scratch.path().join("music.mp3"), &mp3).expect("write the mp3");

    // ⚠️ The control for the whole test. If these bytes were decodable by the
    // linked codec, everything below would pass with the hook removed and prove
    // nothing about the web build.
    assert!(
        gdaudio::decode::decode_mp3(&mp3).is_err(),
        "fixture check: this .mp3 must be undecodable without the browser's PCM"
    );

    // What the page does: decode with the browser, hand over the PCM, install.
    let decoder = BrowserMp3Decoder::new();
    decoder.install();
    assert_eq!(
        decoder
            .register_planar(&mp3, &tone_planar(4.0), DECODED_RATE, 2)
            .expect("register the browser-decoded PCM"),
        1
    );
    assert!(
        gdaudio::decode::installed_mp3_decoder().is_some(),
        "the decoder must be installed in gdaudio's slot, or the loader cannot see it"
    );

    let (mut engine, mut ring) = booted(scratch.path());
    let channels = ring.channels() as usize;
    assert!(channels >= 1, "the ring must report a channel count");

    // A pull before any frame has run must be silent. If this is loud, the pull
    // is mixing a server of its own and the rest proves nothing about the
    // MainLoop.
    let cold = ring.pull_block(QUANTUM);
    assert_eq!(cold.len(), QUANTUM * channels);
    assert_eq!(
        rms(&cold),
        0.0,
        "audio appeared before any frame stepped, so `pull_block` is mixing \
         something the scene does not drive"
    );

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
        engine.main_loop().audio_bridge().stream_errors().is_empty(),
        "the MP3 failed to load even with PCM registered: {:?}",
        engine.main_loop().audio_bridge().stream_errors()
    );
    assert_eq!(
        engine.main_loop().audio_bridge().active_stream_count(),
        1,
        "the scene's autoplaying AudioStreamPlayer should hold a live voice"
    );
    assert!(
        loudest > 0.01,
        "the MP3 the browser decoded never reached the worklet ring (loudest \
         block rms {loudest}). On wasm32 `decode_mp3_frames` is a stub, so this \
         is the only path by which an AudioStreamMP3 can be heard at all."
    );
    assert!(
        engine.main_loop().audio_bridge().last_mix().is_empty(),
        "with an output attached the bridge must write THROUGH it rather than \
         retaining the mix — a non-empty last_mix means the output branch was \
         never taken, so these frames did not travel the worklet path"
    );
}

/// The other side of the claim: with nothing registered, the same scene and the
/// same bytes are silent and the loader says why.
///
/// Without this, "audio came out" would also be satisfied by a loader that
/// quietly synthesised something, and the acceptance above could not tell the
/// difference.
#[test]
fn an_unregistered_mp3_is_silent_and_reports_an_error() {
    let _slot = DecoderSlot::acquire();
    let scratch = Scratch::new("web-mp3-unregistered");
    let mp3 = undecodable_mp3();
    std::fs::write(scratch.path().join("music.mp3"), &mp3).expect("write the mp3");

    // Installed but EMPTY — the state of a page that enabled browser decoding
    // and then forgot to preload. A miss must fall through to the linked codec,
    // which cannot decode these bytes either.
    BrowserMp3Decoder::new().install();

    let (mut engine, mut ring) = booted(scratch.path());
    let channels = ring.channels() as usize;

    engine.frame(ORIGIN);
    let mut stamp = ORIGIN;
    let mut loudest = 0.0f32;
    for _ in 0..12 {
        stamp += FRAME_MS;
        engine.frame(stamp);
        for _ in 0..QUANTA_PER_FRAME {
            let block = ring.pull_block(QUANTUM);
            assert_eq!(block.len(), QUANTUM * channels, "always a full block");
            loudest = loudest.max(rms(&block));
        }
    }

    assert!(
        loudest < 1e-5,
        "an MP3 nothing decoded produced audio (loudest block rms {loudest}); the \
         ring is being fed by something other than the scene's stream"
    );
    assert!(
        !engine.main_loop().audio_bridge().stream_errors().is_empty(),
        "an undecodable stream must be reported, not silently dropped — a page \
         whose music is mute needs to be distinguishable from a scene with no \
         music in it"
    );
}
