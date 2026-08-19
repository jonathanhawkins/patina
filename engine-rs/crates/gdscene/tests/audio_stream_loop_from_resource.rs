//! pat-s3rrv.56 acceptance — a stream that declares a loop keeps playing.
//!
//! Godot's `AudioStreamWAV` carries `loop_mode` (DISABLED/FORWARD/PINGPONG/
//! BACKWARD) plus `loop_begin`/`loop_end` in FRAMES, and playback honours them,
//! so background music authored as looping plays forever
//! (`scene/resources/audio_stream_wav.cpp`).
//!
//! Patina had the whole runtime half already: `gdaudio::stream` implements all
//! four modes with a `[loop_begin, loop_end]` window and the mixer's read cursor
//! respects direction and window. What was missing was every link that could
//! reach it — `set_loop_mode`/`set_loop_begin`/`set_loop_end` had **no caller
//! outside `gdaudio`**, the WAV `smpl` chunk was never parsed, and
//! `AudioStreamInfo` had nowhere to put a loop even if it had been. So every
//! looping track played once and stopped.
//!
//! This drives the shipped path end to end: `.wav` on disk (with a real `smpl`
//! chunk) + `.tscn` -> `PackedScene` -> `SceneTree` -> `MainLoop::step` ->
//! `AudioServerBridge` -> `AudioStreamPlayback` -> mixed PCM.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;

const RATE: u32 = 8_000;
/// 0.2 s at 8 kHz, matching the bead's fixture.
const FRAMES: u32 = RATE / 5;

/// A 0.2 s mono sine at full scale, so "silent" and "playing" are unmistakable.
fn tone_pcm() -> Vec<u8> {
    let mut pcm = Vec::with_capacity(FRAMES as usize * 2);
    for n in 0..FRAMES {
        let t = n as f32 / RATE as f32;
        let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.9;
        pcm.extend_from_slice(&((s * i16::MAX as f32) as i16).to_le_bytes());
    }
    pcm
}

/// A `smpl` chunk declaring one forward loop over the whole sample.
///
/// Layout: 36 bytes of chunk header, then 24 bytes per loop. ⚠️ The loop `type`
/// here is the WAV numbering (0 = forward), NOT Godot's `LoopMode` (where 0 is
/// DISABLED) — the translation is the decoder's job and is what this fixture
/// exercises.
fn smpl_chunk() -> Vec<u8> {
    let mut c = Vec::new();
    c.extend_from_slice(b"smpl");
    c.extend_from_slice(&60u32.to_le_bytes());
    for _ in 0..7 {
        c.extend_from_slice(&0u32.to_le_bytes());
    }
    c.extend_from_slice(&1u32.to_le_bytes()); // one loop
    c.extend_from_slice(&0u32.to_le_bytes()); // sampler data size
    c.extend_from_slice(&0u32.to_le_bytes()); // cue point id
    c.extend_from_slice(&0u32.to_le_bytes()); // type 0 = forward
    c.extend_from_slice(&0u32.to_le_bytes()); // begin frame
    c.extend_from_slice(&(FRAMES - 1).to_le_bytes()); // end frame
    c.extend_from_slice(&0u32.to_le_bytes()); // fraction
    c.extend_from_slice(&0u32.to_le_bytes()); // play count
    c
}

/// The tone as a real RIFF/WAV, with the `smpl` loop chunk when `looping`.
///
/// `build_wav_bytes` emits only `fmt `+`data`, so the loop chunk is appended and
/// the RIFF size patched to cover it.
fn tone_wav(looping: bool) -> Vec<u8> {
    let mut wav = gdaudio::wav::build_wav_bytes(RATE, 1, 16, 1, &tone_pcm());
    if looping {
        wav.extend_from_slice(&smpl_chunk());
        let riff_size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
    }
    wav
}

const SCENE: &str = r#"[gd_scene format=3]

[node name="World" type="Node"]

[node name="Music" type="AudioStreamPlayer" parent="."]
stream = "res://tone.wav"
autoplay = true
"#;

/// Plays the scene for `secs` of simulated time, returning the peak amplitude
/// mixed during the FINAL frame.
///
/// The final frame is what matters: a stream that stopped is silent there while
/// a looping one is not. Sampling the whole run would be dominated by the first
/// 0.2 s, which both cases play identically.
fn peak_at_end(looping: bool, secs: f64) -> f32 {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("tone.wav"), tone_wav(looping)).unwrap();

    let scene = PackedScene::from_tscn(SCENE).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_base_dir(dir.path().to_path_buf());

    let step = 1.0 / 60.0;
    let steps = (secs / step).round() as usize;
    let mut last = 0.0f32;
    for _ in 0..steps {
        ml.step(step);
        last = ml
            .audio_bridge()
            .last_mix()
            .iter()
            .fold(0.0f32, |a, s| a.max(s.abs()));
    }
    last
}

/// **The acceptance test.** A 0.2 s WAV that declares forward looping is still
/// producing audio at 0.9 s; the same scene without the loop is silent.
#[test]
fn audio_stream_loop_from_resource() {
    // Fixture check first: BOTH variants must be audible early, or the
    // comparison below would just be measuring whether the file loaded at all.
    assert!(
        peak_at_end(true, 0.1) > 0.01,
        "fixture: the looping tone must be audible while inside its first pass"
    );
    assert!(
        peak_at_end(false, 0.1) > 0.01,
        "fixture: the unlooped tone must be audible while inside its first pass"
    );

    let looped = peak_at_end(true, 0.9);
    assert!(
        looped > 0.01,
        "a forward-looping 0.2 s stream must still be audible at 0.9 s, got peak \
         {looped:.5}. Silence here means the loop the file declares never reached \
         `AudioStreamPlayback::set_loop_mode` — the setters had no caller outside \
         gdaudio before this bead."
    );

    let once = peak_at_end(false, 0.9);
    assert!(
        once <= 0.01,
        "the SAME scene with no declared loop must be silent at 0.9 s, got peak \
         {once:.5}. If this is loud too, the loop is being applied to every \
         stream rather than to the ones that ask for it — which would pass the \
         assertion above for the wrong reason."
    );
}

/// The same claim structurally: the looping voice is still ALIVE at 0.9 s while
/// the one-shot has ended.
///
/// Peak amplitude alone cannot tell "the voice ended" from "the voice is
/// playing a quiet passage"; the live-voice count can, so the two assertions
/// fail for different reasons and a regression cannot slip past both.
#[test]
fn only_the_looping_voice_survives_past_the_stream_length() {
    fn live_voices_at(looping: bool, secs: f64) -> usize {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("tone.wav"), tone_wav(looping)).unwrap();

        let scene = PackedScene::from_tscn(SCENE).expect("parse tscn");
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");

        let mut ml = MainLoop::new(tree);
        ml.audio_bridge_mut().set_base_dir(dir.path().to_path_buf());
        let step = 1.0 / 60.0;
        for _ in 0..((secs / step).round() as usize) {
            ml.step(step);
        }
        ml.audio_bridge().active_stream_count()
    }

    assert_eq!(
        live_voices_at(true, 0.9),
        1,
        "a forward-looping stream must still have a live voice at 0.9 s"
    );
    assert_eq!(
        live_voices_at(false, 0.9),
        0,
        "a one-shot 0.2 s stream must have ended by 0.9 s"
    );
}

/// The WAV `smpl` chunk reaches `AudioStreamInfo` as SECONDS.
///
/// ⚠️ This is the units trap, pinned deliberately. The `smpl` chunk and Godot's
/// `loop_begin`/`loop_end` are in FRAMES; `set_loop_begin`/`set_loop_end` take
/// SECONDS. Passing frames straight through compiles and yields a loop window
/// hundreds of times too long — on a 0.2 s sample that is indistinguishable
/// from no looping at all.
#[test]
fn wav_loop_points_arrive_in_seconds_not_frames() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("tone.wav");
    std::fs::write(&path, tone_wav(true)).unwrap();

    let import = gdaudio::import::load_audio_file(&path).expect("wav imports");
    let points = import
        .info
        .loop_points
        .expect("the smpl chunk must reach AudioStreamInfo");

    assert_eq!(points.mode, gdaudio::stream::LoopMode::Forward);
    assert!(
        (points.begin_secs - 0.0).abs() < 1e-6,
        "begin: {}",
        points.begin_secs
    );
    // (FRAMES - 1) frames at 8 kHz is ~0.2 s — NOT 1599.0.
    assert!(
        (points.end_secs - 0.199_875).abs() < 1e-3,
        "end must be ~0.2 SECONDS, got {} — a value near {} means frames were \
         passed through as seconds",
        points.end_secs,
        FRAMES - 1
    );
}

/// A WAV with no `smpl` chunk reports no loop, so every existing one-shot
/// keeps playing once.
#[test]
fn a_wav_without_smpl_declares_no_loop() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("plain.wav");
    std::fs::write(&path, tone_wav(false)).unwrap();

    let import = gdaudio::import::load_audio_file(&path).expect("wav imports");
    assert!(
        import.info.loop_points.is_none(),
        "a file that declares no loop must not acquire one"
    );
}
